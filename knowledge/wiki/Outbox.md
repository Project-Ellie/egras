---
title: Outbox
tags:
  - outbox
  - events
  - reliability
  - architecture
---

# Outbox Pattern

Reliable, transaction-coupled publication of domain events. Lives at `src/outbox/` and bridges domain mutations to the [[Jobs]] queue.

## Why

A service that wants to "create user → send welcome email" risks two failure modes if it enqueues the job directly: (a) job enqueued but the user transaction rolls back, leaving an orphan side-effect; or (b) the user commits but the enqueue crashes, losing the side-effect entirely. The outbox collapses both into one transaction by writing the event row to `outbox_events` *inside the same transaction* as the domain change. A separate relayer process later moves committed events into the [[Jobs]] queue, where existing retry / backoff / dead-letter machinery delivers them.

## Two stages — outbox + jobs

- **Outbox = what happened.** Commit-coupled with the domain write. Pure record of facts.
- **Jobs = what to do about it.** Reuses the existing retry, observability, and shutdown machinery without duplication.
- The relayer is the only component that bridges the two, in a single transaction.

## Module layout

| File | Role |
|------|------|
| [`src/outbox/model.rs`](../../src/outbox/model.rs) | `OutboxEvent`, `AppendRequest` |
| [`src/outbox/persistence/mod.rs`](../../src/outbox/persistence/mod.rs) | `OutboxRepository` trait |
| [`src/outbox/persistence/outbox_repository_pg.rs`](../../src/outbox/persistence/outbox_repository_pg.rs) | Postgres impl; also impls `OutboxAppender` |
| [`src/outbox/relayer.rs`](../../src/outbox/relayer.rs) | `OutboxAppender` facade, `OutboxRelayer`, `OutboxRelayerConfig` |
| [`migrations/0010_outbox_events.sql`](../../migrations/0010_outbox_events.sql) | `outbox_events` table + partial index |

## Data model

```
outbox_events (
  id              UUID PK,
  aggregate_type  TEXT,                 -- nullable hint for ops/debugging
  aggregate_id    UUID,                 -- nullable hint for ops/debugging
  event_type      TEXT NOT NULL,        -- routing key → job kind
  payload         JSONB NOT NULL,
  created_at      TIMESTAMPTZ DEFAULT now(),
  relayed_at      TIMESTAMPTZ,          -- NULL until relayer marks it
  relay_attempts  INT DEFAULT 0,
  last_error      TEXT
)
```

Partial index `ix_outbox_unrelayed ON (created_at) WHERE relayed_at IS NULL` keeps the relayer's claim query bounded by backlog size, not by total event history.

## Lifecycle

```
   service tx                       relayer tx
   ──────────                       ──────────
   begin                            begin
   INSERT domain row     ┐          claim_unrelayed_in_tx (FOR UPDATE SKIP LOCKED)
   outbox.append_in_tx   ┘ same tx  jobs.enqueue_in_tx (one per event)
   commit  ───────────────┐         outbox.mark_relayed_in_tx
                          ▼         commit
                     row visible
                     to relayer
```

If the service tx rolls back, no event row exists — nothing to relay. If the relayer tx rolls back, the outbox stays unrelayed and is retried on the next tick. **At-least-once delivery; consumers must be idempotent** (same as [[Jobs]] already requires).

## Appending from a service

Services depend on `Arc<dyn OutboxAppender>` via `AppState::outbox`. The narrow `OutboxAppender` trait exposes only `append_in_tx` — repository internals (claim, mark-relayed, find) stay invisible to business code, mirroring how `JobsEnqueuer` hides `JobsRepository`.

```rust
let mut tx = pool.begin().await?;
// ... domain INSERTs/UPDATEs on `tx` ...
state.outbox.append_in_tx(
    &mut tx,
    AppendRequest::new("user.created", json!({"user_id": user.id}))
        .with_aggregate("user", user.id),
).await?;
tx.commit().await?;
```

A handler for `event_type = "user.created"` registered in the [[Jobs]] runner picks the work up after the relayer bridges it.

## Relayer

`OutboxRelayer` is a long-running task spawned in `build_app`. Each tick:

1. Begin tx.
2. `claim_unrelayed_in_tx` (`FOR UPDATE SKIP LOCKED`, ordered by `created_at`).
3. For each event, `jobs.enqueue_in_tx(tx, EnqueueRequest::now(event_type, payload))`.
4. `mark_relayed_in_tx` on all claimed ids.
5. Commit.

Any failure inside steps 2–4 rolls the tx back, leaving the outbox unchanged. The next tick retries.

`OutboxRelayerConfig` defaults: `poll_interval = 250 ms`, `batch_size = 64`, `job_max_attempts = 5`. No env vars yet.

`SKIP LOCKED` makes the design horizontally scalable: N relayers compete on the same table without double-enqueue.

## Wiring

`build_app`:

1. Constructs one `Arc<OutboxRepositoryPg>`.
2. Coerces it to `Arc<dyn OutboxRepository>` (relayer dependency) and `Arc<dyn OutboxAppender>` (`AppState::outbox`, for services).
3. Spawns the relayer with the jobs repository and config.
4. Returns the `OutboxRelayerHandle` in `AppHandles`.

`main.rs` shuts down in the order: HTTP graceful → outbox relayer → jobs runner → audit worker → pool. The outbox stops first so it stops feeding the jobs runner that we are about to drain.

## Non-goals (v1)

- No per-aggregate ordering guarantees; relayer orders by `created_at`, consumers must be idempotent.
- No fan-out: one event → one job. Multi-subscriber comes with [[future-enhancements/Notification-Channels]].
- No event versioning / schema registry.
- No replay endpoint for ops.
- No DLQ for the outbox itself — if the relayer cannot enqueue jobs (e.g. jobs table broken), the row stays unrelayed forever with `relay_attempts` and `last_error` for visibility. Outbox is the source of truth; losing rows would be wrong.
- No automatic cleanup of relayed rows. Covered by [[future-enhancements/Data-Retention-Policies]].

## Testing

- Persistence: [`tests/outbox_persistence_test.rs`](../../tests/outbox_persistence_test.rs) — append commit/rollback, FIFO order, `SKIP LOCKED` partitioning between concurrent claimers, `mark_relayed_in_tx` on N ids (and empty-input no-op).
- Relayer: [`tests/outbox_relayer_test.rs`](../../tests/outbox_relayer_test.rs) — single-tick relays + marks, enqueue failure rolls back and keeps the outbox unrelayed, end-to-end append → relay → registered job handler runs, graceful shutdown.

Both use `TestPool::fresh()` for isolation.

## Related

- [[Jobs]] — the queue this feeds; retry / backoff / dead-letter live there
- [[Architecture]] — module map
- Original spec: [`docs/superpowers/specs/2026-05-03-outbox-pattern-design.md`](../../docs/superpowers/specs/2026-05-03-outbox-pattern-design.md)
