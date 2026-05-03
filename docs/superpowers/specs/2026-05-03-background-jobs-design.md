---
title: Background Jobs — Durable Worker Subsystem
date: 2026-05-03
status: draft
tags:
  - spec
  - jobs
---

# Background Jobs — Design Spec

## Why

Many planned features (notifications, email, webhooks, GDPR exports, retention purges, bulk import/export, API usage analytics, error fingerprinting) are async work units that must survive process restart, retry on failure, and be observed. The current `audit::worker` is a fire-and-forget in-memory `mpsc` queue — correct for hot-path audit, wrong for everything else. We add a *new* durable jobs subsystem rather than retrofitting audit (which would add latency to every request).

## Non-goals (v1)

- Cron / recurring schedules. Out of scope; callers enqueue jobs themselves.
- Priority lanes / fairness across kinds.
- Multi-language workers. Postgres + Rust only.
- A separate dead-letter table. Failed jobs stay in `jobs` with `state='dead'`.
- Replacing the audit worker. Untouched.

## Data model

`migrations/0009_jobs.sql`:

```sql
CREATE TABLE jobs (
    id              UUID PRIMARY KEY,
    kind            TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    state           TEXT NOT NULL,            -- pending | running | done | dead
    attempts        INT  NOT NULL DEFAULT 0,
    max_attempts    INT  NOT NULL,
    run_at          TIMESTAMPTZ NOT NULL,     -- earliest time to run
    locked_until    TIMESTAMPTZ,              -- visibility timeout
    locked_by       TEXT,                     -- worker id (instance hostname + uuid)
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_jobs_due ON jobs (state, run_at) WHERE state IN ('pending','running');
CREATE INDEX ix_jobs_kind_state ON jobs (kind, state);
```

State transitions: `pending → running → done` (success) or `running → pending` (retry, with backoff via `run_at`) or `running → dead` (attempts exhausted).

## Components

### `src/jobs/model.rs`
`Job`, `JobState`, `EnqueueRequest`. UUID v7 ids. `JobState` is a small enum with `as_str()`/`from_str` for SQL.

### `src/jobs/persistence.rs`
Trait `JobsRepository`:
- `enqueue(EnqueueRequest) -> Result<Uuid>`
- `claim_due(worker_id, kinds: &[String], visibility: Duration, limit: u32) -> Result<Vec<Job>>`
  Uses `UPDATE … FROM (SELECT … FOR UPDATE SKIP LOCKED) …` to atomically lock and mark `running`.
- `mark_done(id) -> Result<()>`
- `mark_failed_retry(id, error: &str, next_run_at) -> Result<()>` (sets state back to `pending`, increments `attempts`, clears lock).
- `mark_dead(id, error: &str) -> Result<()>`
- `find(id) -> Result<Option<Job>>` (test helper).

`JobsRepositoryPg` implements it against Postgres.

### `src/jobs/runner.rs`
`JobHandler` trait (object-safe via `async_trait`):
```rust
#[async_trait]
pub trait JobHandler: Send + Sync {
    fn kind(&self) -> &'static str;
    async fn handle(&self, payload: &serde_json::Value) -> Result<(), JobError>;
}
```

`JobError` distinguishes `Retryable` from `Permanent` (permanent → dead immediately).

`JobRunner` holds:
- `repo: Arc<dyn JobsRepository>`
- `handlers: HashMap<&'static str, Arc<dyn JobHandler>>`
- `worker_id: String`
- config: `poll_interval`, `visibility_timeout`, `batch_size`, `backoff_initial`, `backoff_factor`, `backoff_max`.

`spawn() -> JobRunnerHandle` runs a loop:
1. `claim_due(...)` for registered kinds.
2. For each claimed job: dispatch to handler.
3. Outcome → `mark_done` / `mark_failed_retry(next_run_at = now + backoff)` / `mark_dead`.
4. Sleep `poll_interval` if no work.

Graceful shutdown: handle has a `CancellationToken`; current job finishes, then loop exits. Drop releases held jobs (their `locked_until` will expire and another worker re-claims).

### Wiring
`AppState` gets `jobs: Arc<dyn JobsEnqueuer>` (a thin trait exposing only `enqueue`, so handlers don't take a runner).

`build_app` constructs the runner with registered handlers (none in this PR — registry is the API). `main.rs` keeps the runner handle alive.

## Backoff

`next_run_at = now + min(backoff_max, backoff_initial * backoff_factor^(attempts - 1))`. Defaults: 5s initial, factor 4, max 1h.

## Tests (vertical slices)

1. **Persistence:** enqueue → find → state == pending.
2. **Persistence:** `claim_due` returns one job, marks it running with `locked_until` ~= now+visibility; second concurrent call returns nothing for that id.
3. **Persistence:** scheduled job (`run_at` in future) is not claimed.
4. **Persistence:** lock-expired job (`locked_until` < now, state=running) is re-claimable.
5. **Runner:** noop handler — enqueued job ends in `done`.
6. **Runner:** failing handler — retries up to `max_attempts`, ends in `dead` with `last_error`.
7. **Runner:** permanent error from handler → immediate `dead`.
8. **Runner:** unregistered kind — claimed, handler missing → dead with `"no handler for kind"` error.
9. **Runner:** scheduled job not run before `run_at`.
10. **Runner:** graceful shutdown finishes in-flight handler.

All persistence tests use `TestPool::fresh()`. Runner tests use a real pg + a programmable `MockHandler`.

## Wiki updates (same PR)

- New: `knowledge/wiki/Jobs.md` — module overview, lifecycle diagram, how to add a handler.
- Update: `knowledge/wiki/Architecture.md` — add `jobs/` row to module map.
- Update: `knowledge/wiki/future-enhancements/Background-Jobs.md` → `status: done`, add link to PR, then delete file (per user instruction once happy).

## Open questions

(None — resolved internally:)
- *Multiple runners?* Yes via `claim_due` SKIP LOCKED; v1 ships a single runner instance.
- *Priority?* Out of scope; future field.
- *Cron?* Out of scope; will live in a separate PR atop this.
