---
title: Outbox Pattern — Reliable Domain-Event Publishing
date: 2026-05-03
status: draft
tags:
  - spec
  - outbox
  - events
---

# Outbox Pattern — Design Spec

## Why

Domain mutations and the events they produce must publish atomically. Today, a service that wants to "create user → send welcome email" risks: (a) email enqueued but user transaction rolled back, or (b) user committed but email enqueue crashed. The outbox pattern collapses both into a single transaction by writing the event row to an `outbox_events` table inside the *same* transaction as the domain change. A separate relayer process later moves committed events into the [[Jobs]] queue, where existing retry/DLQ machinery delivers them.

## Why two stages (outbox → jobs)

- **Outbox = what happened.** Commit-coupled with the domain write. Pure record of facts.
- **Jobs = what to do about it.** Reuses the existing retry, backoff, dead-letter, observability machinery without re-implementing.
- The relayer is the ONLY component that bridges them, in a single transaction.

## Non-goals (v1)

- No per-aggregate ordering guarantees (relayer orders by `created_at`; consumers must be idempotent).
- No fan-out to multiple handlers per event (one event → one job; multi-subscriber comes with [[future-enhancements/Notification-Channels]]).
- No event versioning / schema registry.
- No replay endpoint for ops.

## Data model

`migrations/0010_outbox_events.sql`:

```sql
CREATE TABLE outbox_events (
    id              UUID PRIMARY KEY,
    aggregate_type  TEXT,
    aggregate_id    UUID,
    event_type      TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    relayed_at      TIMESTAMPTZ,
    relay_attempts  INT NOT NULL DEFAULT 0,
    last_error      TEXT
);
CREATE INDEX ix_outbox_unrelayed
    ON outbox_events (created_at)
    WHERE relayed_at IS NULL;
```

`aggregate_type/id` are nullable hints for ops/debugging, not used for routing in v1.

## Components

### `src/outbox/model.rs`
`OutboxEvent` (full row) and `AppendRequest` (event_type, payload, optional aggregate hints). UUID v7 generated in Rust.

### `src/outbox/persistence/mod.rs`
```rust
#[async_trait]
pub trait OutboxRepository: Send + Sync + 'static {
    /// Append within an existing transaction. The caller controls commit.
    async fn append_in_tx(
        &self, tx: &mut sqlx::PgConnection, req: AppendRequest,
    ) -> anyhow::Result<Uuid>;

    /// Claim a batch of unrelayed events with FOR UPDATE SKIP LOCKED.
    /// Caller must mark them relayed within the same `tx` to avoid loss.
    async fn claim_unrelayed_in_tx(
        &self, tx: &mut sqlx::PgConnection, limit: u32,
    ) -> anyhow::Result<Vec<OutboxEvent>>;

    /// Mark a set of events as relayed (called inside the relayer tx).
    async fn mark_relayed_in_tx(
        &self, tx: &mut sqlx::PgConnection, ids: &[Uuid],
    ) -> anyhow::Result<()>;

    async fn find(&self, id: Uuid) -> anyhow::Result<Option<OutboxEvent>>;
}
```

### Narrow facade `OutboxAppender` (in `src/outbox/relayer.rs` or `mod.rs`)
```rust
#[async_trait]
pub trait OutboxAppender: Send + Sync + 'static {
    async fn append_in_tx(
        &self, tx: &mut sqlx::PgConnection, req: AppendRequest,
    ) -> anyhow::Result<Uuid>;
}
```
Same shape as `JobsEnqueuer` — service code only sees this. `OutboxRepositoryPg` impls both this and `OutboxRepository`.

### Relayer
`OutboxRelayer` is a long-running task that polls and bridges:

```text
loop:
  begin tx
    events = claim_unrelayed_in_tx(tx, batch_size)   -- FOR UPDATE SKIP LOCKED
    if empty: rollback; sleep(poll_interval); continue
    for e in events:
        jobs.enqueue_in_tx(tx, EnqueueRequest{kind=e.event_type, payload=e.payload, ...})
    mark_relayed_in_tx(tx, ids)
  commit
```

If anything in the body fails, tx rollback leaves outbox unrelayed — relayer retries on next tick. At-least-once delivery; consumers must be idempotent (same as jobs already requires).

### Required jobs API extension
Add `JobsRepository::enqueue_in_tx(&mut PgConnection, EnqueueRequest)`. The existing `enqueue(req)` becomes a thin wrapper that opens its own tx.

## Wiring

- `AppState` adds `outbox: Arc<dyn OutboxAppender>`. Constructed once as a concrete `Arc<OutboxRepositoryPg>` then coerced both to `Arc<dyn OutboxRepository>` (for the relayer) and `Arc<dyn OutboxAppender>` (for services).
- `build_app` constructs the relayer with a poll interval and batch size, spawns it, and returns the handle in `AppHandles`.
- `main.rs` shuts the relayer down before the audit/jobs handles.

## Configuration

`OutboxRelayerConfig` defaults: `poll_interval = 250ms`, `batch_size = 64`. No env vars yet.

## Tests (vertical slices)

1. **Persistence:** `append_in_tx` then commit → row visible.
2. **Persistence:** `append_in_tx` then rollback → row absent.
3. **Persistence:** `claim_unrelayed_in_tx` returns events in `created_at` ascending order.
4. **Persistence:** concurrent relayers (two txs) — each claims a disjoint subset due to `SKIP LOCKED`.
5. **Persistence:** `mark_relayed_in_tx` on N ids sets all `relayed_at`.
6. **Relayer:** append event, run one tick → corresponding job enqueued, outbox row marked relayed.
7. **Relayer:** simulate jobs.enqueue failure inside the tx → outbox row stays unrelayed.
8. **Relayer:** registered handler for `event_type` runs end-to-end (append → relay → job → handler).
9. **Relayer:** graceful shutdown completes.

## Wiki updates (same PR)

- New: `knowledge/wiki/Outbox.md` — pattern overview, lifecycle, how to append from a service.
- Update: `knowledge/wiki/Architecture.md` — add `outbox/` row to module map.
- Update: `knowledge/wiki/Jobs.md` — cross-link "fed by outbox for tx-coupled events".
- Update: `knowledge/wiki/future-enhancements/INDEX.md` — strike through Outbox-Pattern.
- Delete: `knowledge/wiki/future-enhancements/Outbox-Pattern.md`.

## Open questions

(None — resolved internally:)
- *Per-aggregate ordering?* Out of scope; idempotency on the consumer covers it.
- *DLQ for outbox itself?* Outbox doesn't have one — if relayer can't enqueue jobs, the row stays unrelayed and retried forever (with `relay_attempts++` and `last_error` for visibility). This is correct: outbox is the source of truth.
- *Cleanup of relayed rows?* Out of scope — covered by [[future-enhancements/Data-Retention-Policies]].
