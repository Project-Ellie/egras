---
title: Data Model
tags:
  - database
  - schema
  - migrations
---

# Data Model

All schema is managed via sqlx migrations, applied automatically at startup. Migration files live in [`migrations/`](../../migrations/).

## Entity Relationship Overview

```
organisations ──< user_organisation_roles >── users
     │                      │
     │                    roles ──< role_permissions >── permissions
     │
     └── audit_events (target_organisation_id)

users ──< password_reset_tokens
users ──< revoked_tokens (via jwt revocation)
audit_events (actor_user_id → users)
```

## Tables

### `organisations`

Defined in [`migrations/0002_tenants.sql`](../../migrations/0002_tenants.sql).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7, application-generated |
| `name` | `text` UNIQUE NOT NULL | Tenant display name |
| `business` | `text` NOT NULL | Industry or business type |
| `is_operator` | `bool` DEFAULT false | Exactly one operator row |
| `created_at` | `timestamptz` | Auto-set |
| `updated_at` | `timestamptz` | Auto-updated via trigger |

The **operator organisation** is seeded in [`migrations/0005_seed_operator_and_rbac.sql`](../../migrations/0005_seed_operator_and_rbac.sql) with a deterministic ID: `00000000-0000-0000-0000-000000000001`. Users with membership in the operator org and the `operator_admin` role get elevated permissions across all tenants — see [[Authorization]].

### `users`

Defined in [`migrations/0003_security.sql`](../../migrations/0003_security.sql).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7 |
| `username` | `text` UNIQUE NOT NULL | |
| `email` | `citext` UNIQUE NOT NULL | Case-insensitive (PostgreSQL `citext` extension) |
| `password_hash` | `text` NOT NULL | Argon2id PHC string |
| `created_at` | `timestamptz` | |
| `updated_at` | `timestamptz` | |

`citext` is enabled in [`migrations/0001_extensions.sql`](../../migrations/0001_extensions.sql). It means `SELECT * FROM users WHERE email = 'Alice@Example.com'` matches `alice@example.com` transparently.

### `roles`

Defined in [`migrations/0004_rbac.sql`](../../migrations/0004_rbac.sql).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7 |
| `code` | `text` UNIQUE NOT NULL | Human-readable identifier |
| `is_builtin` | `bool` | True for the 4 seeded roles |
| `created_at` | `timestamptz` | |

**Built-in roles** (seeded in migration 0005):

| Code | Scope |
|------|-------|
| `operator_admin` | Operator org only; has wildcard-like permissions over all tenants |
| `org_owner` | Full control within one org (can delete org, remove members) |
| `org_admin` | Manage members and roles within one org |
| `org_member` | Basic read access within one org |

### `permissions`

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7 |
| `code` | `text` UNIQUE NOT NULL | Dotted code, e.g. `tenants.create` |
| `created_at` | `timestamptz` | |

**All permission codes:**

| Code | Description |
|------|-------------|
| `tenants.manage_all` | Full cross-tenant administrative access |
| `users.manage_all` | Register/manage users across all tenants |
| `tenants.create` | Create a new organisation |
| `tenants.update` | Update org name/business |
| `tenants.read` | Read org details |
| `tenants.members.add` | Add users to an org |
| `tenants.members.remove` | Remove users from an org |
| `tenants.members.list` | List members of an org |
| `tenants.roles.assign` | Assign roles to org members |
| `channels.manage` | Manage inbound channels for an organisation |
| `audit.read_all` | Read audit events from any org |
| `audit.read_own_org` | Read audit events of own org only |

### `role_permissions`

Join table: `role_id` → `permission_id`. Many-to-many.

### Permission matrix

|  | `operator_admin` | `org_owner` | `org_admin` | `org_member` |
|--|:---:|:---:|:---:|:---:|
| `tenants.manage_all` | ✓ | | | |
| `users.manage_all` | ✓ | | | |
| `tenants.create` | ✓ | ✓ | | |
| `tenants.update` | ✓ | ✓ | | |
| `tenants.read` | ✓ | ✓ | ✓ | ✓ |
| `tenants.members.add` | ✓ | ✓ | ✓ | |
| `tenants.members.remove` | ✓ | ✓ | ✓ | |
| `tenants.members.list` | ✓ | ✓ | ✓ | ✓ |
| `tenants.roles.assign` | ✓ | ✓ | ✓ | |
| `channels.manage` | ✓ | ✓ | ✓ | |
| `audit.read_all` | ✓ | | | |
| `audit.read_own_org` | ✓ | ✓ | ✓ | |

### `user_organisation_roles`

Join table for user membership.

| Column | Type | Notes |
|--------|------|-------|
| `user_id` | `uuid` FK | → `users.id` CASCADE DELETE |
| `organisation_id` | `uuid` FK | → `organisations.id` CASCADE DELETE |
| `role_id` | `uuid` FK | → `roles.id` |
| `created_at` | `timestamptz` | Used as `joined_at` for display |

PK: `(user_id, organisation_id, role_id)`. A user can hold multiple roles in the same org.

### `password_reset_tokens`

Defined in [`migrations/0003_security.sql`](../../migrations/0003_security.sql).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7 |
| `token_hash` | `text` UNIQUE NOT NULL | SHA-256 hex of the raw token |
| `user_id` | `uuid` FK | → `users.id` CASCADE DELETE |
| `expires_at` | `timestamptz` NOT NULL | Configurable TTL (default 3600s) |
| `consumed_at` | `timestamptz` | Set when token is used; NULL = unused |
| `created_at` | `timestamptz` | |

The raw token is never stored. Only the hash is persisted. See [[Security-Domain#Password Reset]] for the full flow.

### `revoked_tokens`

Defined in [`migrations/0007_revoked_tokens.sql`](../../migrations/0007_revoked_tokens.sql).

| Column | Type | Notes |
|--------|------|-------|
| `jti` | `text` PK | JWT ID claim (UUIDv7 string) |
| `user_id` | `uuid` FK | → `users.id` CASCADE DELETE |
| `expires_at` | `timestamptz` NOT NULL | Copied from JWT `exp`; used for index range scan |

When a user logs out, the token's `jti` is inserted here. The [[Authentication#Middleware|auth middleware]] checks this table on every request. Old rows can be cleaned up once `expires_at` has passed (no token with that JTI could be valid anyway).

### `audit_events`

Defined in [`migrations/0006_audit.sql`](../../migrations/0006_audit.sql). Append-only — no UPDATEs or DELETEs.

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUIDv7 |
| `occurred_at` | `timestamptz` NOT NULL | Actual event timestamp |
| `category` | `text` NOT NULL | e.g., `security.auth` |
| `event_type` | `text` NOT NULL | e.g., `login.success` |
| `actor_user_id` | `uuid` | Who performed the action |
| `actor_organisation_id` | `uuid` | In which org context |
| `target_type` | `text` | `user`, `organisation`, etc. |
| `target_id` | `uuid` | What was affected |
| `target_organisation_id` | `uuid` | Which tenant |
| `request_id` | `text` | Tracing correlation ID |
| `ip_address` | `inet` | Client IP (nullable) |
| `user_agent` | `text` | |
| `outcome` | `text` NOT NULL | `success`, `failure`, `denied` |
| `reason_code` | `text` | e.g., `bad_password`, `missing:tenants.create` |
| `payload` | `jsonb` | Custom per-event-type data |

Indexes: `occurred_at`, `target_organisation_id`, `actor_user_id`, `event_type`.

For the full event model, see [[Audit-System]].

## `inbound_channels`

Per-organisation ingress endpoints. Defined in [`migrations/0008_inbound_channels.sql`](../../migrations/0008_inbound_channels.sql).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | UUID v7 |
| `organisation_id` | `uuid` FK | → `organisations.id` CASCADE DELETE |
| `name` | `text` | UNIQUE per `(organisation_id, name)` |
| `description` | `text` | nullable |
| `channel_type` | `text` | CHECK in `('vast','sensor','websocket','rest')` |
| `api_key` | `text` | 64-char hex; generated server-side; never reissued |
| `is_active` | `boolean` | DEFAULT TRUE |
| `created_at` / `updated_at` | `timestamptz` | |

Index: `inbound_channels_organisation_id_name_key` (UNIQUE).

## Migrations

Migrations are applied at startup via `sqlx::migrate!`. They are ordered and non-destructive:

| File | Contents |
|------|---------|
| [`0001_extensions.sql`](../../migrations/0001_extensions.sql) | `CREATE EXTENSION citext` |
| [`0002_tenants.sql`](../../migrations/0002_tenants.sql) | `organisations` table |
| [`0003_security.sql`](../../migrations/0003_security.sql) | `users`, `password_reset_tokens` |
| [`0004_rbac.sql`](../../migrations/0004_rbac.sql) | `roles`, `permissions`, `role_permissions`, `user_organisation_roles` |
| [`0005_seed_operator_and_rbac.sql`](../../migrations/0005_seed_operator_and_rbac.sql) | Operator org, 4 built-in roles, permissions, role-permission mappings |
| [`0006_audit.sql`](../../migrations/0006_audit.sql) | `audit_events` + indexes |
| [`0007_revoked_tokens.sql`](../../migrations/0007_revoked_tokens.sql) | `revoked_tokens` + `expires_at` index |
| [`0008_inbound_channels.sql`](../../migrations/0008_inbound_channels.sql) | `inbound_channels` + `channels.manage` permission |

> [!warning] Migration 0005 is idempotent
> `INSERT ... ON CONFLICT DO NOTHING` is used throughout seed migration 0005, so re-running migrations is safe.

## ID Strategy

All IDs are **UUID v7** — generated by the application, never by the database. UUID v7 is time-ordered, which means:
- B-tree indexes stay efficient (no fragmentation)
- IDs are sortable by creation time
- Cursor-based pagination uses `(occurred_at, id)` tuples without ambiguity

See [[Pagination]] for how IDs are used in cursors.

## Related notes

- [[Architecture]] — how the persistence layer fits into the overall structure
- [[Authentication]] — how `revoked_tokens` is used during auth
- [[Authorization]] — how `user_organisation_roles` and `role_permissions` drive RBAC
- [[Audit-System]] — full detail on `audit_events`
- [[Security-Domain]] — `users` and `password_reset_tokens` in context
- [[Tenants-Domain]] — `organisations`, roles, and memberships in context
