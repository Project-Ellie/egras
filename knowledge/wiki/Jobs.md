---
title: Jobs
tags:
  - jobs
  - async
  - architecture
---

# Background Jobs

Durable, retryable, Postgres-backed work queue. Lives at `src/jobs/` and is wired through `AppState`. Decoupled from [[Audit-System]] — the audit worker remains an in-memory `mpsc` channel for hot-path latency; jobs is for everything else (email, webhook delivery, exports, retention purges, GDPR DSAR, API analytics rollups, etc.).

## Module layout

| File | Role |
|------|------|
| [`src/jobs/model.rs`](../../src/jobs/model.rs) | `Job`, `JobState`, `EnqueueRequest` |
| [`src/jobs/persistence/mod.rs`](../../src/jobs/persistence/mod.rs) | `JobsRepository` trait |
| [`src/jobs/persistence/jobs_repository_pg.rs`](../../src/jobs/persistence/jobs_repository_pg.rs) | Postgres implementation; also impls `JobsEnqueuer` |
| [`src/jobs/runner.rs`](../../src/jobs/runner.rs) | `JobHandler` trait, `JobRunner`, `JobRunnerConfig`, `JobError`, `JobsEnqueuer` |
| [`migrations/0009_jobs.sql`](../../migrations/0009_jobs.sql) | `jobs` table + indexes |

## Data model

```
jobs (
  id            UUID PK,
  kind          TEXT,               -- handler kind, e.g. "email.send"
  payload       JSONB,
  state         TEXT,               -- pending | running | done | dead
  attempts      INT,
  max_attempts  INT,
  run_at        TIMESTAMPTZ,        -- earliest dispatch time
  locked_until  TIMESTAMPTZ,        -- visibility timeout while running
  locked_by     TEXT,               -- worker id
  last_error    TEXT,
  created_at    TIMESTAMPTZ,
  updated_at    TIMESTAMPTZ
)
```

Indexes: `(state, run_at) WHERE state IN ('pending','running')` for the claim hot path; `(kind, state)` for ops queries.

## Lifecycle

```
            enqueue
              │
              ▼
       ┌──────────────┐  claim_due   ┌──────────────┐
       │   pending    │─────────────▶│   running    │
       └──────┬───────┘              └──────┬───────┘
              │ retry                       │
   mark_failed_retry                  ┌─────┴──────┐
   (next_run_at = now + backoff)      ▼            ▼
              │                  mark_done    mark_dead
              │                       │            │
              └───────────────────────▼────────────▼
                                   done         dead
```

- `claim_due` uses `FOR UPDATE SKIP LOCKED` so multiple runners can scale horizontally without double-claim.
- A crashed worker's lock expires after `visibility_timeout`; another worker re-claims.
- `JobError::Permanent` short-circuits retries straight to `dead`.
- `JobError::Retryable` increments `attempts` and reschedules with exponential backoff capped at `backoff_max`. Reaching `max_attempts` transitions to `dead`.

## Enqueueing from a service

Services depend on `Arc<dyn JobsEnqueuer>` via `AppState::jobs` — the narrow facade exposes only `enqueue`, hiding `claim_due` / `mark_*` from business code.

```rust
state.jobs.enqueue(EnqueueRequest::now(
    "email.send",
    serde_json::json!({"to": "user@example.com", "template": "welcome"}),
)).await?;
```

## Adding a handler

1. Implement `JobHandler` for your handler struct (`async fn handle(&self, payload) -> Result<(), JobError>`).
2. Choose a stable `kind` string — used as the routing key and as the source of truth in stored rows.
3. Register it in [`build_app`](../../src/lib.rs) before spawning the runner: `JobRunner::new(repo, cfg).register(Arc::new(MyHandler::new(...)))`.
4. Decide retry semantics: return `JobError::Retryable` for transient (network, lock contention) and `JobError::Permanent` for non-retryable (bad payload, gone-resource).
5. Add tests at the persistence layer (claim/mark behavior) and the runner level (handler outcomes).

The runner currently has zero handlers registered — this is the platform; producers and consumers land per feature.

## Configuration

`JobRunnerConfig` with defaults:
- `poll_interval`: 500 ms (idle backoff between empty `claim_due` calls)
- `visibility_timeout`: 60 s (lock held while a job is running)
- `batch_size`: 16 (max jobs claimed per poll)
- `backoff_initial`: 5 s, `backoff_factor`: 4, `backoff_max`: 1 h

No env vars yet — change defaults in code if needed; expose via `AppConfig` once usage demands.

## Testing

- Persistence: [`tests/jobs_persistence_test.rs`](../../tests/jobs_persistence_test.rs) — enqueue, claim, scheduled-not-yet, lock expiry, mark transitions.
- Runner: [`tests/jobs_runner_test.rs`](../../tests/jobs_runner_test.rs) — success, retry-to-dead, permanent shortcut, idle behavior, graceful shutdown. Uses a `CountingHandler` that records call counts.

Both use `TestPool::fresh()` for isolation.

## Why a new system, not a generalised audit worker

Audit events fire from the HTTP hot path. Persisting through a polled Postgres queue would add a write + poll latency to every audited request. Two systems with different cost profiles serve their workloads better than one compromised generalisation.

## Known limits / future work

- No cron / recurring schedules — callers enqueue future-dated jobs themselves; a scheduler can layer on top.
- No priority lanes; a single FIFO order by `run_at`.
- `dead` rows are not auto-purged; use the future [[future-enhancements/Data-Retention-Policies]] mechanism.
- Single in-process runner today; the schema and `SKIP LOCKED` already support N runners — the deployment shape just hasn't required it.

## Related

- [[Architecture]] — module map
- [[Audit-System]] — the in-memory worker contrasted above
- [[future-enhancements/Outbox-Pattern]] — sits on top of this for at-least-once event publishing
- Original spec: [`docs/superpowers/specs/2026-05-03-background-jobs-design.md`](../../docs/superpowers/specs/2026-05-03-background-jobs-design.md)
