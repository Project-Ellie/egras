---
title: Audit System
tags:
  - audit
  - events
  - async
---

# Audit System

egras maintains a comprehensive immutable audit trail. Every state-changing operation and every authentication event writes an `AuditEvent`. The write path is non-blocking — events are queued and persisted by a background worker.

## Architecture

```
Handler calls:
  state.audit_recorder.record(event).await
         │
         ▼
ChannelAuditRecorder
  ├─ Mirror to tracing log immediately
  │   (target: "egras::audit", structured JSON, level: INFO)
  └─ try_send() to bounded mpsc channel
         │
         │   (non-blocking — if channel is full, event is dropped + error logged)
         ▼
AuditWorker  (spawned tokio task)
  ├─ Drain from mpsc receiver
  ├─ For each event:
  │   ├─ INSERT INTO audit_events ...
  │   ├─ On failure: exponential backoff + retry
  │   │   (configurable: EGRAS_AUDIT_MAX_RETRIES, EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL)
  │   └─ On permanent failure: log error, drop event
  └─ On channel close: drain remaining events, then exit
```

The two key traits are defined in [`src/audit/service/`](../../src/audit/service/):

- `AuditRecorder` — `async fn record(&self, event: AuditEvent)` — production impl: `ChannelAuditRecorder`
- `ListAuditEvents` — `async fn list_events(&self, filter: &AuditQueryFilter) → Result<AuditPage>`

In tests, `BlockingAuditRecorder` writes directly to the DB synchronously, enabling deterministic assertions. See [[Testing-Strategy#BlockingAuditRecorder]].

## AuditEvent Model

Defined in [`src/audit/model.rs`](../../src/audit/model.rs):

```rust
pub struct AuditEvent {
    pub id:                     Uuid,                // UUIDv7
    pub occurred_at:            DateTime<Utc>,
    pub category:               AuditCategory,
    pub event_type:             String,
    pub actor_user_id:          Option<Uuid>,
    pub actor_organisation_id:  Option<Uuid>,
    pub target_type:            Option<String>,      // "user", "organisation", etc.
    pub target_id:              Option<Uuid>,
    pub target_organisation_id: Option<Uuid>,
    pub request_id:             Option<String>,
    pub ip_address:             Option<String>,      // INET
    pub user_agent:             Option<String>,
    pub outcome:                Outcome,
    pub reason_code:            Option<String>,
    pub payload:                serde_json::Value,   // Custom per-event-type JSON
}
```

### Outcome

```rust
pub enum Outcome {
    Success,
    Failure,
    Denied,
}
```

### AuditCategory

```rust
pub enum AuditCategory {
    SecurityStateChange,     // "security.state_change"
    SecurityAuth,            // "security.auth"
    SecurityPermissionDenial,// "security.permission_denial"
    DataAccess,              // "data.access"
    TenantsStateChange,      // "tenants.state_change"
}
```

## Event Constructors

All events are created via named constructors on `AuditEvent` in [`src/audit/model.rs`](../../src/audit/model.rs), rather than constructing structs directly. This keeps event shape consistent and documents what each event type carries.

| Constructor | Category | event_type | Emitted by |
|------------|----------|-----------|-----------|
| `login_success(user_id, org_id)` | `SecurityAuth` | `login.success` | `login` service |
| `login_failed(reason, username)` | `SecurityAuth` | `login.failed` | `login` service |
| `logout(user_id, org_id, jti)` | `SecurityAuth` | `logout` | `logout` service |
| `session_switched_org(user_id, from, to)` | `SecurityAuth` | `session.switched_org` | `switch_org` service |
| `user_registered_success(actor, actor_org, user_id, target_org, role)` | `SecurityStateChange` | `user.registered` | `register_user` service |
| `password_changed(user_id)` | `SecurityStateChange` | `password.changed` | `change_password` service |
| `password_reset_requested(user_id)` | `SecurityStateChange` | `password.reset_requested` | `password_reset_request` service |
| `password_reset_confirmed(user_id, outcome)` | `SecurityStateChange` | `password.reset_confirmed` | `password_reset_confirm` service |
| `permission_denied(user_id, org_id, code)` | `SecurityPermissionDenial` | `permission.denied` | auth middleware |
| `organisation_created(actor, actor_org, org_id, name)` | `TenantsStateChange` | `organisation.created` | `create_organisation` service |
| `member_added(actor, actor_org, user_id, org_id, role)` | `TenantsStateChange` | `organisation.member_added` | `add_user_to_organisation` service |
| `member_removed(actor, actor_org, user_id, org_id)` | `TenantsStateChange` | `organisation.member_removed` | `remove_user_from_organisation` service |
| `role_assigned(actor, actor_org, user_id, org_id, role)` | `TenantsStateChange` | `organisation.role_assigned` | `assign_role` service |
| `users_list(actor, actor_org)` | `DataAccess` | `users.list` | `list_users` service |
| `admin_seeded(user_id, org_id, role)` | `SecurityStateChange` | `user.registered` | `seed-admin` CLI |

## Querying Audit Events

Endpoint: `GET /api/v1/audit/events`  
Requires: `audit.read_all` OR `audit.read_own_org`

| Parameter | Type | Description |
|-----------|------|-------------|
| `organisation_id` | UUID | Filter by target org (required if only `audit.read_own_org`) |
| `actor_user_id` | UUID | Filter by who performed the action |
| `event_type` | string | Exact match, e.g. `login.success` |
| `category` | string | e.g. `security.auth` |
| `outcome` | string | `success`, `failure`, or `denied` |
| `from` | ISO 8601 | Events at or after this timestamp |
| `to` | ISO 8601 | Events before this timestamp |
| `after` | string | Opaque cursor for next-page pagination |
| `limit` | integer | Page size (default 10, max 100) |

Response shape:

```json
{
  "items": [
    {
      "id": "...",
      "occurred_at": "2026-01-01T12:00:00Z",
      "category": "security.auth",
      "event_type": "login.success",
      "actor_user_id": "...",
      "actor_organisation_id": "...",
      "outcome": "success",
      "payload": {}
    }
  ],
  "next_cursor": "eyJvY2N1cnJlZF9hdCI6Ii4uLiIsImlkIjoiLi4uIn0"
}
```

`next_cursor` is an opaque base64url-encoded JSON cursor — see [[Pagination]] for details. Pass it as `after=<cursor>` to fetch the next page. If `null`, there are no more results.

### Org-scoping rules for audit reads

- Caller with `audit.read_all` → can query any `organisation_id` or no filter
- Caller with only `audit.read_own_org` → `organisation_id` must equal `claims.org_id`; cross-org queries return 403

## Tracing Integration

Every audit event is also mirrored to the `tracing` log at `INFO` level, structured as JSON:

```json
{
  "target": "egras::audit",
  "level": "INFO",
  "event_type": "login.success",
  "category": "security.auth",
  "actor_user_id": "...",
  "outcome": "success"
}
```

This means even if the DB write fails (e.g., worker queue full), the event is still visible in log aggregators (Datadog, CloudWatch, etc.). Log and DB audit serve different guarantees — logs are best-effort, DB is persistent with retry.

## Worker Configuration

| Env var | Default | Effect |
|---------|---------|--------|
| `EGRAS_AUDIT_CHANNEL_CAPACITY` | 4096 | mpsc channel buffer size |
| `EGRAS_AUDIT_MAX_RETRIES` | 3 | DB write retry count |
| `EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL` | 100 | Initial backoff ms; doubles each retry |

If the channel fills (all 4096 slots occupied), new events are **dropped** and an error is logged. This is intentional — audit writes must never block HTTP response times. Size the channel generously.

## Related notes

- [[Architecture]] — where the audit worker fits in the application lifecycle
- [[Data-Model#`audit_events`]] — the database table schema
- [[Testing-Strategy#BlockingAuditRecorder]] — how tests assert on audit events
- [[Design-Decisions#Non-blocking audit]] — why the channel+worker pattern was chosen
