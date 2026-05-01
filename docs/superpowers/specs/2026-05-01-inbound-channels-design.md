# InboundChannel CRUD — Design Spec

Date: 2026-05-01  
Status: Approved

## Overview

Each organisation can maintain any number of `InboundChannel` records. A channel is reference data describing a source of inbound signals (REST, VAST, WebSocket, sensor feed, etc.) and carries a server-generated API key used to authenticate that signal source. This feature adds pure CRUD maintenance for those records; signal-processing logic is out of scope.

## Data Model

New struct added to `src/tenants/model.rs`:

```rust
pub struct InboundChannel {
    pub id: Uuid,                    // UUIDv7, app-generated
    pub organisation_id: Uuid,
    pub name: String,                // 1–120 chars, unique within org
    pub description: Option<String>, // up to 1000 chars
    pub channel_type: ChannelType,
    pub api_key: String,             // plaintext, server-generated at creation
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum ChannelType { Vast, Sensor, Websocket, Rest }
```

- `id` is UUIDv7 (app-generated, never DB-generated), consistent with all other entities.
- `api_key` is server-generated at creation as a random 32-byte value encoded as a 64-character lowercase hex string. The client cannot supply their own key. Key rotation is out of scope.
- `(organisation_id, name)` has a unique constraint — two channels in the same org cannot share a name.
- `api_key` is stored plaintext; no additional confidentiality measures are applied at this layer (additional authentication is handled elsewhere).

## Migration

`migrations/0008_inbound_channels.sql` contains:

1. `inbound_channels` table with all fields above.
2. Unique index on `(organisation_id, name)`.
3. New permission row: `('00000000-0000-0000-0000-00000000020c', 'channels.manage', 'Manage inbound channels for an organisation')`.
4. Role → permission grants: `operator_admin`, `org_owner`, `org_admin` all receive `channels.manage`. `org_member` does not.

## API Endpoints

All routes registered under the existing `/api/v1/tenants` prefix, added to `src/tenants/interface.rs`.

| Method   | Path                                              | Status | Description                          |
|----------|---------------------------------------------------|--------|--------------------------------------|
| `POST`   | `/organisations/:org_id/channels`                 | 201    | Create channel; response includes generated `api_key` |
| `GET`    | `/organisations/:org_id/channels`                 | 200    | List channels (cursor-paginated)     |
| `GET`    | `/organisations/:org_id/channels/:channel_id`     | 200    | Fetch single channel                 |
| `PUT`    | `/organisations/:org_id/channels/:channel_id`     | 200    | Update name/description/type/is_active; `api_key` is immutable |
| `DELETE` | `/organisations/:org_id/channels/:channel_id`     | 204    | Hard delete                          |

Pagination on the list endpoint follows the existing cursor pattern (keyed on `created_at` + `id`).

Cross-org access uses the existing `authorise_org` helper: operator users (holding `tenants.manage_all`) pass through; all others get `404` if `org_id` mismatches their JWT `org`.

## Permissions & RBAC

Single new permission: `channels.manage`.

New extractor `ChannelsManage` in `src/auth/extractors.rs` implementing the `Permission` trait. Its `accepts` method returns true if the caller holds `channels.manage` OR `tenants.manage_all`, consistent with the operator-wildcard pattern used by other tenant-scoped permissions.

All five endpoints require `Perm<ChannelsManage>`.

## Service Layer

Five use-case files under `src/tenants/service/`:

- `create_inbound_channel.rs` — generates UUIDv7 id and random `api_key`, inserts, emits audit event.
- `list_inbound_channels.rs` — cursor-paginated query scoped to `organisation_id`.
- `get_inbound_channel.rs` — single fetch; returns `404` if not found or wrong org.
- `update_inbound_channel.rs` — updates mutable fields, bumps `updated_at`, emits audit event.
- `delete_inbound_channel.rs` — hard delete, emits audit event.

## Persistence Layer

Repository trait `InboundChannelRepository` and PostgreSQL implementation `InboundChannelRepositoryPg` under `src/tenants/persistence/`, following the existing `organisation_repository` / `role_repository` pattern. Injected via `AppState`.

## Audit

Audit events emitted on create, update, and delete. Read operations (list, get) are not audited, consistent with existing conventions.

## Testing

Tests organized under `tests/tenants/` at three layers:

- **Persistence:** insert/read/update/delete via `TestPool::fresh()`.
- **Service:** unit-level logic (duplicate name → conflict error, wrong-org → not found).
- **Interface:** full HTTP round-trip with `reqwest` against a bound local port; covers permission denial (403), org isolation (404), and happy-path CRUD.

## Error Cases

| Condition | Error |
|-----------|-------|
| Duplicate `(org_id, name)` | `409 Conflict`, slug `channel_name_taken` |
| Channel not found or wrong org | `404 Not Found`, slug `channel_not_found` |
| Validation failure (name length, etc.) | `400 Bad Request`, slug `validation_error` |
| Missing or insufficient permission | `403 Forbidden` |
