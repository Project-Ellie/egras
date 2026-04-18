---
title: egras — Rust Application Seed Design
date: 2026-04-18
status: approved
tags: [spec, architecture, rust, axum, postgres]
---

# egras — Enterprise-Ready Rust-based Application Seed

## 1. Purpose & Scope

`egras` is a seed for Rust backend services exposing a REST API. Its purpose is to encode an opinionated set of patterns (tech stack, architecture, authN/authZ, auditing, testing, ops) so that feature work starts from a known-good baseline rather than from scratch.

The seed implements three domains end-to-end — **security** (user management), **tenants** (organisation management), and **audit** (record + query of security-relevant events) — so that every layer is exercised by real, production-shaped use cases and so that future domains have clear templates to copy.

### In scope

- Axum-based HTTP service over a Tokio runtime
- PostgreSQL persistence via sqlx
- JWT (HS256) authentication with per-request organisation scope
- Full RBAC: DB-backed roles, permission codes, role-permission mappings
- Three horizontal domains (security, tenants, audit) with four vertical layers each (interface, service, model, persistence)
- Use-case-as-a-service REST style (no CRUD semantics, verbs in paths)
- Comprehensive audit trail: state-changing actions, authentication outcomes, and permission denials across all domains; persisted to Postgres and mirrored to structured logs; queryable via the audit domain
- Layered automated test strategy (persistence, service, interface, end-to-end)
- OpenAPI auto-generation via utoipa
- Container- and CI-ready (Dockerfile, docker-compose.yml, GitHub Actions)
- Operator super-tenant with cross-organisation privileges
- Bootstrap CLI subcommand `seed-admin` for the first operator admin

### Out of scope (explicit non-goals)

- Email delivery (SMTP) — password reset stubs to logs only
- Rate limiting
- Refresh tokens / JWT revocation list / session management
- Multi-language / i18n
- GraphQL, WebSocket, gRPC surfaces
- OpenTelemetry, Prometheus metrics
- Soft-delete semantics (hard-delete with FK cascade is used)
- Read-access auditing (only state-changing actions, auth outcomes, and permission denials are audited)
- Tamper-evident audit (hash-chaining or signing of audit rows)
- Audit export API, retention, or archival jobs

## 2. Tech Stack

| Concern | Choice | Notes |
|---|---|---|
| Language / runtime | Rust (stable, edition 2021) with Tokio | |
| HTTP framework | Axum + tower-http | |
| DB driver | sqlx with Postgres | Compile-time checked queries allowed but not required |
| Migrations | `sqlx::migrate!("./migrations")` | Ordered SQL files, run at startup |
| JSON | serde, serde_json | snake_case by default |
| Validation | `validator` crate | Applied to request DTOs |
| Password hashing | `argon2` crate, argon2id | Library-recommended parameters |
| JWT | `jsonwebtoken` crate, HS256 | Single symmetric secret from env |
| UUIDs | `uuid` crate, v7 | Application-generated, time-ordered |
| Timestamps | `chrono::DateTime<Utc>`, Postgres `timestamptz` | RFC 3339 in JSON |
| CLI | `clap` (derive) | Subcommand dispatch in `main.rs` |
| Config | `figment` (env provider, optional `.env`) | |
| Logging / tracing | `tracing` + `tracing_subscriber` | JSON layer by default |
| Async channels | `tokio::sync::mpsc` (bounded) | Audit worker queue |
| OpenAPI | `utoipa`, `utoipa-swagger-ui` | `/docs` + `/api-docs/openapi.json` |
| Containers (tests) | `testcontainers-rs` | Ephemeral Postgres for persistence + e2e |
| Mocking | `mockall` | Generated mocks of repository + service traits |
| HTTP client (e2e) | `reqwest` | Against the bound local port |
| CI | GitHub Actions, single Ubuntu job | fmt / clippy / test / build |

## 3. Architecture

### 3.1 Two-dimensional layout

- **Horizontal domains** (use-case contexts): `security`, `tenants`, `audit`. Future domains are added as sibling modules following the same internal shape.
- **Vertical layers** inside each domain:
  1. `interface` — Axum handlers and DTOs. Thin adapters only.
  2. `service` — use cases. One Rust file per use case, each defining a trait and a concrete implementation.
  3. `model` — domain entities, value objects, role/permission types.
  4. `persistence` — repository traits and sqlx-backed implementations.

Cross-cutting concerns (`auth`, `errors`, `config`, `db`, `app_state`) live outside the domain modules.

### 3.2 Wiring — trait objects everywhere

All service and repository types are `pub` traits. Implementations are concrete structs. `AppState` holds `Arc<dyn Trait + Send + Sync>` for every service and for the `AuditRecorder`. Handlers depend on traits, never on concrete impls.

```rust
pub struct AppState {
    // security
    pub register_user:            Arc<dyn RegisterUser>,
    pub login:                    Arc<dyn Login>,
    pub change_password:          Arc<dyn ChangePassword>,
    // ... one per use case ...
    // tenants
    pub create_organisation:      Arc<dyn CreateOrganisation>,
    pub add_user_to_organisation: Arc<dyn AddUserToOrganisation>,
    // ...
    // audit
    pub audit_recorder:           Arc<dyn AuditRecorder>,
    pub list_audit_events:        Arc<dyn ListAuditEvents>,
}
```

Every service that emits audit events depends on `Arc<dyn AuditRecorder>` and calls `record(event)` at outcome points. The recorder never blocks on DB writes (see §7.3).

### 3.3 Use case = file

Each use case lives in its own source file under `src/<domain>/service/<use_case>.rs`. The file defines:

- A request struct (internal to service layer; distinct from interface DTO)
- A result type and a domain-specific error enum (or a variant of `AppError`)
- A `#[async_trait]` trait describing the use case
- A concrete `*Impl` struct holding `Arc<dyn …Repository>` dependencies
- An `impl` block for the trait on the concrete struct

Handlers in `interface.rs` read the trait from `AppState`, parse the DTO, call the trait method, map the result to an HTTP response.

### 3.4 Authorisation middleware

```
Request
  └─ AuthLayer (tower::Layer)
       ├─ Extract Authorization: Bearer <jwt>
       ├─ Decode + validate (exp, iss, typ)
       ├─ Load permission codes for (user_id, org_id) as PermissionSet
       └─ Insert Claims + PermissionSet into request extensions
  └─ Handler
       └─ RequirePermission("<code>") / AuthorisedOrg extractors
           └─ On 403: emit AuditEvent::PermissionDenied via AuditRecorder
```

Unauthenticated endpoints (`/health`, `/ready`, `/docs`, `/api-docs/openapi.json`, `/api/v1/security/login`, `/api/v1/security/password-reset-request`, `/api/v1/security/password-reset-confirm`) are registered on a sub-router that skips `AuthLayer`.

### 3.5 Cross-organisation rule (operator bypass)

When evaluating a handler that receives an `organisation_id`:

1. If the caller's `PermissionSet` contains any `*.manage_all` or `audit.read_all` code, the handler proceeds regardless of the JWT's `org_id`.
2. Otherwise, the handler enforces `organisation_id == claims.org_id`; mismatch returns **404** (not 403) to avoid leaking existence of other tenants' resources.

## 4. Project Layout

```
egras/
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
├── .github/workflows/ci.yml
├── .env.example
├── docs/
│   └── openapi.json                        # committed; CI checks drift
├── migrations/
│   ├── 0001_extensions.sql
│   ├── 0002_tenants.sql
│   ├── 0003_security.sql
│   ├── 0004_rbac.sql
│   ├── 0005_seed_operator_and_rbac.sql
│   └── 0006_audit.sql
├── src/
│   ├── main.rs                             # clap: serve (default) | seed-admin | dump-openapi
│   ├── lib.rs                              # pub fn build_app(pool, config) -> (Router, AuditWorkerHandle)
│   ├── config.rs
│   ├── db.rs
│   ├── errors.rs
│   ├── app_state.rs
│   ├── testing.rs                          # #[cfg(any(test, feature = "testing"))]
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── jwt.rs
│   │   ├── middleware.rs
│   │   └── permissions.rs
│   ├── security/
│   │   ├── mod.rs
│   │   ├── interface.rs
│   │   ├── service/
│   │   │   ├── mod.rs
│   │   │   ├── register_user.rs
│   │   │   ├── login.rs
│   │   │   ├── logout.rs
│   │   │   ├── change_password.rs
│   │   │   ├── switch_org.rs
│   │   │   ├── password_reset_request.rs
│   │   │   └── password_reset_confirm.rs
│   │   ├── model.rs
│   │   └── persistence/
│   │       ├── mod.rs
│   │       ├── user_repository_pg.rs
│   │       └── token_repository_pg.rs
│   ├── tenants/
│   │   ├── mod.rs
│   │   ├── interface.rs
│   │   ├── service/
│   │   │   ├── mod.rs
│   │   │   ├── create_organisation.rs
│   │   │   ├── add_user_to_organisation.rs
│   │   │   ├── remove_user_from_organisation.rs
│   │   │   ├── list_my_organisations.rs
│   │   │   ├── list_organisation_members.rs
│   │   │   └── assign_role.rs
│   │   ├── model.rs
│   │   └── persistence/
│   │       ├── mod.rs
│   │       ├── organisation_repository_pg.rs
│   │       └── role_repository_pg.rs
│   └── audit/
│       ├── mod.rs
│       ├── interface.rs                    # list-audit-events handler
│       ├── service/
│       │   ├── mod.rs
│       │   ├── record_event.rs             # AuditRecorder trait + ChannelAuditRecorder impl
│       │   └── list_audit_events.rs
│       ├── model.rs                        # AuditEvent, AuditCategory, Outcome, Actor
│       ├── worker.rs                       # AuditWorker: drains mpsc, writes to repo, retries
│       └── persistence/
│           ├── mod.rs                      # AuditRepository trait
│           └── audit_repository_pg.rs
└── tests/
    ├── common/
    │   ├── mod.rs
    │   ├── fixtures.rs
    │   └── auth.rs
    ├── security/
    │   ├── service/
    │   │   ├── register_user_test.rs
    │   │   ├── login_test.rs
    │   │   ├── logout_test.rs
    │   │   ├── change_password_test.rs
    │   │   ├── switch_org_test.rs
    │   │   ├── password_reset_request_test.rs
    │   │   └── password_reset_confirm_test.rs
    │   ├── persistence/
    │   │   ├── user_repository_pg_test.rs
    │   │   └── token_repository_pg_test.rs
    │   └── interface/
    │       ├── register_test.rs
    │       ├── login_test.rs
    │       ├── logout_test.rs
    │       ├── change_password_test.rs
    │       ├── switch_org_test.rs
    │       ├── password_reset_request_test.rs
    │       └── password_reset_confirm_test.rs
    ├── tenants/
    │   ├── service/
    │   │   ├── create_organisation_test.rs
    │   │   ├── add_user_to_organisation_test.rs
    │   │   ├── remove_user_from_organisation_test.rs
    │   │   ├── list_my_organisations_test.rs
    │   │   ├── list_organisation_members_test.rs
    │   │   └── assign_role_test.rs
    │   ├── persistence/
    │   │   ├── organisation_repository_pg_test.rs
    │   │   └── role_repository_pg_test.rs
    │   └── interface/
    │       ├── create_organisation_test.rs
    │       ├── add_user_to_organisation_test.rs
    │       ├── remove_user_from_organisation_test.rs
    │       ├── list_my_organisations_test.rs
    │       ├── list_organisation_members_test.rs
    │       └── assign_role_test.rs
    ├── audit/
    │   ├── service/
    │   │   ├── record_event_test.rs
    │   │   └── list_audit_events_test.rs
    │   ├── persistence/
    │   │   └── audit_repository_pg_test.rs
    │   └── interface/
    │       └── list_audit_events_test.rs
    └── e2e/
        ├── register_login_switch_org.rs
        ├── operator_cross_tenant.rs
        ├── rbac_enforcement.rs
        ├── password_reset_roundtrip.rs
        ├── bootstrap_seed_admin.rs
        └── audit_trail.rs
```

### 4.1 Rust integration-test constraints

Files in `tests/` can only access the **public** API of the crate. Therefore:

- `build_app`, all service traits, repository traits, DTOs, `AppState`, `AppError`, `AuditRecorder`, and `AuditEvent` types are `pub`.
- A `testing` Cargo feature enables the `testing` module which exposes mock builders, a JWT-minting helper, a `TestApp` harness, a `TestPool` using testcontainers, and a `BlockingAuditRecorder` that writes audit events synchronously to the DB (so tests can read audit state deterministically without waiting for the worker).
- `Cargo.toml` dev-dependencies include the crate itself with `features = ["testing"]`:
  ```toml
  [features]
  testing = []
  [dev-dependencies]
  egras = { path = ".", features = ["testing"] }
  ```

## 5. Domain Model & Database Schema

### 5.1 Entities

| Entity | Purpose |
|---|---|
| `organisations` | Tenants; one row flagged `is_operator = TRUE` |
| `users` | Humans with username, email, password hash |
| `roles` | Named collections of permissions; built-in + custom |
| `permissions` | Atomic permission codes |
| `role_permissions` | Many-to-many between roles and permissions |
| `user_organisation_roles` | Join: a user's role(s) within an organisation |
| `password_reset_tokens` | Hashed reset tokens with TTL |
| `audit_events` | Append-only record of state changes, auth outcomes, and permission denials |

### 5.2 DDL

```sql
-- 0001_extensions.sql
CREATE EXTENSION IF NOT EXISTS citext;
```

```sql
-- 0002_tenants.sql
CREATE TABLE organisations (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    business        TEXT NOT NULL,
    is_operator     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX ux_organisations_operator
    ON organisations (is_operator) WHERE is_operator = TRUE;
```

```sql
-- 0003_security.sql
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE,
    email           CITEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,           -- argon2id encoded string
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE password_reset_tokens (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,    -- SHA-256 hex of raw token
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_prt_user ON password_reset_tokens (user_id);
```

```sql
-- 0004_rbac.sql
CREATE TABLE roles (
    id              UUID PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    description     TEXT,
    is_builtin      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE permissions (
    id              UUID PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,
    description     TEXT
);

CREATE TABLE role_permissions (
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id   UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE user_organisation_roles (
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organisation_id UUID NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, organisation_id, role_id)
);
CREATE INDEX ix_uor_user ON user_organisation_roles (user_id);
CREATE INDEX ix_uor_org  ON user_organisation_roles (organisation_id);
```

```sql
-- 0006_audit.sql
CREATE TABLE audit_events (
    id                       UUID PRIMARY KEY,
    occurred_at              TIMESTAMPTZ NOT NULL,
    recorded_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    category                 TEXT NOT NULL,    -- 'security.state_change' | 'security.auth'
                                               -- | 'security.permission_denial'
                                               -- | 'tenants.state_change'
    event_type               TEXT NOT NULL,    -- e.g., 'user.registered', 'login.failed'
    actor_user_id            UUID,
    actor_organisation_id    UUID,
    target_type              TEXT,             -- 'user' | 'organisation' | 'role'
    target_id                UUID,
    target_organisation_id   UUID,             -- for scoping queries; indexed
    request_id               TEXT,
    ip_address               INET,
    user_agent               TEXT,
    outcome                  TEXT NOT NULL,    -- 'success' | 'failure' | 'denied'
    reason_code              TEXT,
    payload                  JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX ix_audit_occurred_at   ON audit_events (occurred_at DESC);
CREATE INDEX ix_audit_target_org    ON audit_events (target_organisation_id, occurred_at DESC);
CREATE INDEX ix_audit_actor         ON audit_events (actor_user_id, occurred_at DESC);
CREATE INDEX ix_audit_event_type    ON audit_events (event_type, occurred_at DESC);
```

### 5.3 Seed migration (0005)

Inserts:

- One row in `organisations` with `name = 'operator'`, `business = 'Platform Operator'`, `is_operator = TRUE`, deterministic UUID `00000000-0000-0000-0000-000000000001`.
- Built-in roles (all `is_builtin = TRUE`): `operator_admin`, `org_owner`, `org_admin`, `org_member`.
- Permission rows for the codes listed below (including audit permissions).
- Role-permission mappings per the matrix in §5.4.

### 5.4 Built-in permission matrix

| Permission code | operator_admin | org_owner | org_admin | org_member |
|---|:---:|:---:|:---:|:---:|
| `tenants.manage_all` | ✅ | | | |
| `users.manage_all` | ✅ | | | |
| `tenants.create` | ✅ | ✅ | | |
| `tenants.update` | ✅ | ✅ | | |
| `tenants.read` | ✅ | ✅ | ✅ | ✅ |
| `tenants.members.add` | ✅ | ✅ | ✅ | |
| `tenants.members.remove` | ✅ | ✅ | ✅ | |
| `tenants.members.list` | ✅ | ✅ | ✅ | ✅ |
| `tenants.roles.assign` | ✅ | ✅ | ✅ | |
| `audit.read_all` | ✅ | | | |
| `audit.read_own_org` | ✅ | ✅ | ✅ | |

`users.change_own_password` is implicit for any authenticated user and is not modelled in the DB.

## 6. Authentication & Authorisation

### 6.1 JWT claims

```json
{
  "sub":  "<user_uuid>",
  "org":  "<org_uuid>",
  "iat":  1713400000,
  "exp":  1713403600,
  "jti":  "<uuid_v7>",
  "iss":  "egras",
  "typ":  "access"
}
```

- Algorithm: HS256.
- Signing key: `EGRAS_JWT_SECRET`, ≥ 32 bytes, validated on startup.
- TTL: configurable via `EGRAS_JWT_TTL_SECS` (default 3600).

### 6.2 Login

`POST /api/v1/security/login` with `{ username_or_email, password }`.

1. Fetch user by `username` OR `email` (single query, case-insensitive on email via `citext`).
2. Verify password against stored argon2id hash in constant time. Failure → `401 auth.invalid_credentials`. Do not distinguish "user not found" from "wrong password" externally; log distinction for operators; emit audit event `login.failed` with reason code.
3. Fetch all `(organisation_id, role_codes[])` memberships for the user.
4. If **zero** memberships → `403 user.no_organisation`; emit audit event `login.failed` with `reason_code = "no_organisation"`.
5. Pick default org: oldest `created_at` in `user_organisation_roles`.
6. Issue JWT scoped to default org; emit audit event `login.success`. Response:
   ```json
   {
     "access_token": "<jwt>",
     "active_organisation_id": "<uuid>",
     "memberships": [
       { "organisation_id": "<uuid>", "name": "acme", "role_codes": ["org_owner"] },
       { "organisation_id": "<uuid>", "name": "operator", "role_codes": ["operator_admin"] }
     ]
   }
   ```
7. Opportunistic rehash: if stored hash parameters differ from current argon2 defaults, rehash after successful verification and update the user row.

### 6.3 Switch-org

`POST /api/v1/security/switch-org` with `{ organisation_id }`, authenticated.

1. Verify the caller (`sub`) is a member of the requested `organisation_id`; membership missing → `403 permission.denied`.
2. Issue a new JWT scoped to the new `organisation_id`. Old JWT is not revoked; it simply expires.
3. Emit audit event `session.switched_org`.
4. Response:
   ```json
   { "access_token": "<jwt>", "active_organisation_id": "<uuid>" }
   ```

### 6.4 Registration (invited-only)

`POST /api/v1/security/register` with `{ username, email, password, invited_to_organisation_id, role_code? }`.

- Requires the authenticated caller to hold either `users.manage_all` **or** `tenants.members.add` within `invited_to_organisation_id`.
- `role_code` defaults to `org_member`. If supplied, it must be a built-in role and the caller must hold a permission that authorises assigning at least that role (enforced via `tenants.roles.assign` + operator bypass).
- Creates the user (argon2id-hashed password) and inserts a row into `user_organisation_roles`.
- Emits audit event `user.registered`.
- Response: `201 { user_id, organisation_id, assigned_role_code }`.

### 6.5 Logout

`POST /api/v1/security/logout`. Server-stateless; logs `jti` at INFO for audit; emits audit event `logout`. Client is expected to discard the token.

### 6.6 Password change

`POST /api/v1/security/change-password` with `{ current_password, new_password }`, authenticated.

1. Verify `current_password` against stored hash.
2. Hash `new_password` (argon2id) and update user row.
3. Emit audit event `password.changed`.
4. Response: `204`.

### 6.7 Password reset (log-only stub)

- `POST /api/v1/security/password-reset-request` with `{ email }`.
  - Always returns `204` (no user enumeration).
  - If the email exists and has not had too many recent tokens, generate 32 random bytes, store SHA-256 hex of the token with TTL `EGRAS_PASSWORD_RESET_TTL_SECS` (default 3600s), log the reset URL at INFO: `event="password_reset_issued" reset_url="https://.../reset?token=<raw>"`, emit audit event `password.reset_requested`.
- `POST /api/v1/security/password-reset-confirm` with `{ token, new_password }`.
  - SHA-256 the raw token, look up by hash, verify not consumed and not expired.
  - Set `consumed_at = now()`; update user password hash.
  - Emit audit event `password.reset_confirmed` (outcome success/failure with reason code).
  - Response: `204`.

## 7. Audit

### 7.1 Storage

- **Source of truth**: Postgres table `audit_events` (schema in §5.2, migration `0006_audit.sql`).
- **Mirror**: same event emitted as a `tracing` event at INFO for external log pipelines.
- **Durability semantics**: writes happen *after* the business-change transaction commits, via a bounded in-memory queue drained by a background worker with retry. Events can be lost if the process crashes between commit and audit write. This trade-off is explicit.

### 7.2 Domain structure

`src/audit/` mirrors the shape of other domains:

- `interface.rs` — `list-audit-events` handler.
- `service/record_event.rs` — defines `AuditRecorder` trait and `ChannelAuditRecorder` production impl (enqueues on `tokio::sync::mpsc::Sender`, never awaits DB).
- `service/list_audit_events.rs` — query use case.
- `model.rs` — `AuditEvent` (enum over all event variants), `AuditCategory`, `Outcome`, `Actor`.
- `worker.rs` — `AuditWorker` owns the receiver and an `Arc<dyn AuditRepository>`; spawned at startup; runs until shutdown signal.
- `persistence/audit_repository_pg.rs` — sqlx insert + cursor-paginated query.

Services in `security` and `tenants` take `Arc<dyn AuditRecorder>` as a dependency and emit events at use-case outcome points (see §6 for where each event is emitted). Permission-denial events are emitted by the `RequirePermission` extractor in `src/auth/permissions.rs` before returning 403.

### 7.3 Worker & backpressure

- Channel capacity `EGRAS_AUDIT_CHANNEL_CAPACITY` (default 4096).
- Retry: up to `EGRAS_AUDIT_MAX_RETRIES` (default 3) attempts per event with exponential backoff starting at `EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL` (default 100ms, ×4 each attempt).
- On channel full (backpressure): event is dropped and an ERROR-level log emitted with the serialised event.
- On final retry failure: ERROR log with serialised event; worker continues with the next event.
- Graceful shutdown: server stops accepting connections, sender is closed, worker drains the remaining queue, pool closes. `AuditWorkerHandle` returned from `build_app` exposes a `shutdown().await` method.

### 7.4 Event catalogue

**security.state_change**: `user.registered`, `password.changed`, `password.reset_requested`, `password.reset_confirmed`

**security.auth**: `login.success`, `login.failed` (reason codes: `invalid_credentials`, `no_organisation`), `logout`, `session.switched_org`

**security.permission_denial**: `permission.denied` (reason code format: `missing:<permission_code>`)

**tenants.state_change**: `organisation.created`, `organisation.member_added`, `organisation.member_removed`, `organisation.role_assigned`

**Not audited in this seed**: successful read-only actions (list-members, list-organisations, list-audit-events).

### 7.5 Query endpoint

`POST /api/v1/audit/list-audit-events`, authenticated.

Permission: `audit.read_own_org` OR `audit.read_all`.

Request body (all filters optional except pagination defaults):

```json
{
  "organisation_id": "<uuid>|null",
  "actor_user_id":   "<uuid>|null",
  "event_type":      "user.registered|null",
  "category":        "security.auth|null",
  "outcome":         "success|failure|denied|null",
  "from":            "2026-04-01T00:00:00Z|null",
  "to":              "2026-04-18T00:00:00Z|null",
  "cursor":          "<opaque>|null",
  "limit":           100
}
```

Authorisation rule:

- If caller holds `audit.read_all` (operator_admin): `organisation_id` may be any value or null (null ⇒ all orgs).
- If caller holds only `audit.read_own_org`: `organisation_id` must equal the caller's JWT `org_id` or be null (null ⇒ resolved to JWT `org_id` server-side). A mismatched `organisation_id` returns `404 resource.not_found`, consistent with §3.5.

Response shape matches other list endpoints:

```json
{
  "items": [
    {
      "id": "<uuid>",
      "occurred_at": "2026-04-18T12:34:56Z",
      "category": "tenants.state_change",
      "event_type": "organisation.member_added",
      "actor_user_id": "<uuid>",
      "actor_organisation_id": "<uuid>",
      "target_type": "user",
      "target_id": "<uuid>",
      "target_organisation_id": "<uuid>",
      "outcome": "success",
      "reason_code": null,
      "payload": { "role_code": "org_member" }
    }
  ],
  "next_cursor": "<opaque>|null"
}
```

## 8. HTTP API Surface

All endpoints: `Content-Type: application/json`; responses `application/json` (success) or `application/problem+json` (errors). JSON uses snake_case. Every response carries `X-Request-Id`.

### 8.1 Security domain — `/api/v1/security`

| Method | Path | Auth | Permission | Body | Success |
|---|---|---|---|---|---|
| POST | `/register` | yes | `users.manage_all` OR `tenants.members.add` in target org | `{ username, email, password, invited_to_organisation_id, role_code? }` | 201 |
| POST | `/login` | no | — | `{ username_or_email, password }` | 200 |
| POST | `/logout` | yes | — | — | 204 |
| POST | `/change-password` | yes | — | `{ current_password, new_password }` | 204 |
| POST | `/switch-org` | yes | — (membership) | `{ organisation_id }` | 200 |
| POST | `/password-reset-request` | no | — | `{ email }` | 204 |
| POST | `/password-reset-confirm` | no | — | `{ token, new_password }` | 204 |

### 8.2 Tenants domain — `/api/v1/tenants`

| Method | Path | Auth | Permission | Body | Success |
|---|---|---|---|---|---|
| POST | `/create-organisation` | yes | `tenants.create` | `{ name, business }` | 201 — caller becomes `org_owner` |
| POST | `/add-user-to-organisation` | yes | `tenants.members.add` in target org | `{ organisation_id, user_id, role_code }` | 204 |
| POST | `/remove-user-from-organisation` | yes | `tenants.members.remove` in target org | `{ organisation_id, user_id }` | 204 — refuses to remove last owner (409) |
| POST | `/list-my-organisations` | yes | — | `{ cursor?, limit? }` default 50, max 200 | 200 |
| POST | `/list-organisation-members` | yes | `tenants.members.list` | `{ organisation_id, cursor?, limit? }` | 200 |
| POST | `/assign-role` | yes | `tenants.roles.assign` in target org | `{ organisation_id, user_id, role_code, revoke_existing? }` | 204 |

### 8.3 Audit domain — `/api/v1/audit`

| Method | Path | Auth | Permission | Body | Success |
|---|---|---|---|---|---|
| POST | `/list-audit-events` | yes | `audit.read_own_org` OR `audit.read_all` | see §7.5 | 200 |

### 8.4 Operational endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | Liveness; returns `200 { "status": "ok" }`, never touches DB |
| GET | `/ready` | Readiness; acquires a pool connection and `SELECT 1`; 200 on success, 503 on failure |
| GET | `/docs` | utoipa-swagger-ui |
| GET | `/api-docs/openapi.json` | Machine-readable OpenAPI 3.1 spec |

### 8.5 Pagination

List endpoints use cursor-based pagination.

- Request: `{ cursor?: string, limit?: int }`. `limit` defaults to 50 (100 for audit), max 200.
- Response: `{ items: [...], next_cursor?: string }`. `next_cursor` is absent when exhausted.
- Cursor is an opaque base64url-encoded tuple `(created_at_iso, id_uuid)` (or `(occurred_at_iso, id_uuid)` for audit) used in SQL as `WHERE (created_at, id) < (c1, c2) ORDER BY created_at DESC, id DESC LIMIT $n`.

### 8.6 Error response (RFC 7807)

```json
{
  "type":       "https://egras.dev/errors/permission.denied",
  "title":      "Permission denied",
  "status":     403,
  "detail":     "missing permission: tenants.members.add",
  "instance":   "/api/v1/tenants/add-user-to-organisation",
  "request_id": "01HR..."
}
```

For validation errors, the body additionally includes an `errors` map (field → array of error codes), extending the RFC 7807 shape.

Canonical error type slugs:

`validation.invalid_request`, `auth.unauthenticated`, `auth.invalid_credentials`, `permission.denied`, `resource.not_found`, `resource.conflict`, `user.no_organisation`, `rate_limited`, `internal.error`.

### 8.7 Authentication header

`Authorization: Bearer <jwt>`. Missing/invalid → `401 auth.unauthenticated`.

## 9. Configuration

All configuration via environment variables, loaded through `figment`. A `.env` file is supported in dev only.

| Env var | Required | Default | Purpose |
|---|:---:|---|---|
| `EGRAS_DATABASE_URL` | ✅ | — | Postgres connection string |
| `EGRAS_DATABASE_MAX_CONNECTIONS` | | `10` | sqlx pool size |
| `EGRAS_BIND_ADDRESS` | | `0.0.0.0:8080` | HTTP bind addr |
| `EGRAS_JWT_SECRET` | ✅ | — | HS256 key, ≥ 32 bytes |
| `EGRAS_JWT_TTL_SECS` | | `3600` | Access token lifetime |
| `EGRAS_JWT_ISSUER` | | `egras` | `iss` claim |
| `EGRAS_LOG_LEVEL` | | `info` | tracing EnvFilter directive |
| `EGRAS_LOG_FORMAT` | | `json` | `json` \| `pretty` |
| `EGRAS_CORS_ALLOWED_ORIGINS` | | *(empty)* | Comma-separated origins |
| `EGRAS_PASSWORD_RESET_TTL_SECS` | | `3600` | Reset token validity |
| `EGRAS_OPERATOR_ORG_NAME` | | `operator` | Name of the seeded operator org |
| `EGRAS_AUDIT_CHANNEL_CAPACITY` | | `4096` | Bounded queue size for audit worker |
| `EGRAS_AUDIT_MAX_RETRIES` | | `3` | Attempts before giving up per event |
| `EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL` | | `100` | Starting backoff, ×4 per attempt |

Missing required vars → process exits non-zero with a diagnostic naming the offending variable(s). `EGRAS_JWT_SECRET` is validated for minimum length on load.

A `.env.example` is checked into the repo listing every variable.

## 10. Startup & CLI

### 10.1 `egras serve` (default subcommand)

1. Parse CLI + load `AppConfig`.
2. Build `PgPool`.
3. `sqlx::migrate!("./migrations").run(&pool)` — idempotent.
4. Defensive bootstrap check: `SELECT 1 FROM organisations WHERE is_operator = TRUE`. If absent, insert the operator row (should already exist via migration `0005`); log `warn` if inserted at runtime.
5. Create audit mpsc channel (capacity from config); spawn `AuditWorker` task on the Tokio runtime; obtain `AuditWorkerHandle`.
6. Build `AppState` by instantiating each repository and service (concrete structs wrapped in `Arc`), wiring `ChannelAuditRecorder` into the services that need it.
7. Compose `Router` (security routes + tenants routes + audit routes + ops routes + openapi + auth middleware on the protected sub-router).
8. Bind TcpListener to `EGRAS_BIND_ADDRESS`.
9. Serve with graceful shutdown on SIGTERM/SIGINT:
   - Stop accepting new connections.
   - Drain inflight HTTP requests (default grace: 30s).
   - Close the audit mpsc sender; await `AuditWorkerHandle::shutdown()` which drains the remaining queue with one final retry pass.
   - Close the pool.
   - Exit 0.

### 10.2 `egras seed-admin`

```
egras seed-admin --email <email> --username <username> --password <password>
                 [--role <role_code>]      # default: operator_admin
```

Behaviour:

1. Load `AppConfig`; build pool; run migrations.
2. Assert operator org exists (fail with clear message if not).
3. Refuse if a user with `--email` already exists (non-destructive).
4. Hash password (argon2id), insert user, insert `user_organisation_roles (user, operator_org, role)`.
5. This subcommand does NOT spawn the audit worker; it writes a single `user.registered` audit row synchronously using the repository directly (actor = null / "system").
6. Print `user_id` to stdout; exit 0. Never echo secrets.

### 10.3 `egras dump-openapi`

Writes the OpenAPI JSON to stdout. Used by CI to diff against the checked-in `docs/openapi.json`. Exit 0 always (consumer handles diff).

## 11. Test Strategy

### 11.1 Layers

| Layer | Location | Boundary | Proves |
|---|---|---|---|
| Persistence | `tests/<domain>/persistence/*_test.rs` | testcontainers Postgres | SQL correctness, constraints, repository trait contract |
| Service | `tests/<domain>/service/*_test.rs` | mockall repository mocks (and, for security/tenants services, a mock `AuditRecorder`) | Use-case orchestration, error mapping, audit emission |
| Interface | `tests/<domain>/interface/*_test.rs` | mockall service mocks; Axum Router driven via `tower::ServiceExt::oneshot` | Request parsing, auth/permission guards, response shapes, permission-denial audit emission |
| End-to-end | `tests/e2e/*.rs` | Full `build_app` bound to ephemeral port + testcontainers Postgres + `reqwest` + `BlockingAuditRecorder` | Full flows across domains, including audit trail assertions |

### 11.2 File mapping

Tests mirror `src/` one-to-one. Each use-case source file has service and interface companion tests; each persistence module has a persistence companion test. E2E tests are organised by flow, not by endpoint.

### 11.3 Test helpers (`src/testing.rs` under feature `testing`)

- `TestApp::spawn(pool, config) -> TestApp { base_url, shutdown }` — binds to port 0, returns URL. Uses `BlockingAuditRecorder` so audit rows are visible immediately.
- `TestPool::fresh() -> PgPool` — spins up testcontainers Postgres, runs migrations.
- `MockAppStateBuilder` — fluent builder producing an `AppState` with all mocks (including a default mock `AuditRecorder` that records calls for assertion), allowing per-test overrides.
- `mint_jwt(user_id, org_id, permissions, ttl_secs)` — issues a JWT *and* preloads the permission set so interface tests do not require a DB.
- `BlockingAuditRecorder::new(Arc<dyn AuditRepository>)` — synchronous DB-writing recorder used by E2E; bypasses the channel + worker.
- `fixtures::operator_admin()`, `fixtures::org_owner(org_id)`, etc.

### 11.4 Acceptance criteria (minimum tests required for "done")

**Persistence — per repository:**
- Happy-path insert + lookup.
- Unique constraint violation returns the expected repository error.
- FK violation returns the expected repository error.
- Cursor pagination returns correct page and terminates *(only for repositories that expose a paginated query)*.

**Service — per use case:**
- Happy path.
- Each error branch of the use case's error enum is exercised at least once.
- Repository calls use the trait only (asserted via mock expectations).
- For every use case listed in §7.4 as an audit source: a mock `AuditRecorder` expectation asserting the correct `AuditEvent` variant, outcome, and target is emitted (on both success and failure paths where applicable).

**Interface — per endpoint:**
- Unauthenticated request → 401 `auth.unauthenticated`.
- Authenticated but missing permission → 403 `permission.denied` AND a mock `AuditRecorder` expectation asserting a `permission.denied` audit event with the missing code.
- Malformed body → 400 `validation.invalid_request`.
- Happy path round-trips the DTO.
- Error responses conform to RFC 7807 shape.

**E2E — named flows:**

1. `register_login_switch_org` — operator admin registers two users in two different orgs; each logs in, switches org, sees only their resources. Asserts audit rows for each step.
2. `operator_cross_tenant` — operator admin lists members of any org without being a member; regular org owner cannot; a denial row appears in audit for the owner's attempt.
3. `rbac_enforcement` — a user promoted from `org_member` to `org_admin` can subsequently add members; a user demoted loses the permission on the very next request.
4. `password_reset_roundtrip` — request → capture reset URL from the test's `tracing` subscriber → confirm with new password → login with new password succeeds, old password fails. Audit rows: `password.reset_requested`, `password.reset_confirmed`, `login.failed` (old pw), `login.success` (new pw).
5. `bootstrap_seed_admin` — running `egras seed-admin` against an empty DB creates the operator admin user; subsequent `egras serve` login with those credentials succeeds. Audit row for the seed operation exists.
6. `audit_trail` — after a chain of state-changing actions across both domains, the operator_admin `list-audit-events` returns the full sequence in order; an `org_admin` query from another org returns only its own events; a failed login and a permission denial each appear with the correct `outcome` and `reason_code`.

### 11.5 CI

`.github/workflows/ci.yml` — single job, Ubuntu, Docker available:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all-features`
4. `cargo run --release -- dump-openapi > target/openapi.json && diff docs/openapi.json target/openapi.json`
5. `cargo build --release`

## 12. Containers & Local Dev

### 12.1 Dockerfile (multi-stage)

```dockerfile
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/egras /usr/local/bin/egras
ENV EGRAS_BIND_ADDRESS=0.0.0.0:8080
EXPOSE 8080
USER 1000:1000
ENTRYPOINT ["egras"]
CMD ["serve"]
```

### 12.2 docker-compose.yml

- Service `postgres`: `postgres:16-alpine`, named volume for data, healthcheck using `pg_isready`.
- Service `egras`: built from local Dockerfile, `depends_on: postgres: service_healthy`, env vars for local development (secret values committed in the compose file are obviously non-production placeholders).
- `docker compose up` brings up the full stack. `docker compose run --rm egras seed-admin --email admin@example.com --username admin --password changeMe123!` seeds the first admin.

## 13. Observability

- `tracing` with `tracing_subscriber`; JSON layer by default, `pretty` when `EGRAS_LOG_FORMAT=pretty`.
- `tower_http::trace::TraceLayer` on the router. Each request opens a span with fields:
  `request_id`, `method`, `path`, `status`, `latency_ms`, `user_id` (if authenticated), `org_id` (if authenticated).
- `request_id`: from `X-Request-Id` header if present; otherwise generated as UUID v7.
- Every audit event emitted via `AuditRecorder` is also logged at INFO with structured fields matching the DB row columns, so log-based pipelines can observe audit in addition to querying the DB.
- No metrics/OTEL integration in this seed; structured logs are sufficient for verification of E2E tests.

## 14. Error Handling Boundary

- `AppError` is a `thiserror`-derived enum with variants for each canonical error slug.
- `AppError` implements `axum::response::IntoResponse`, emitting RFC 7807 JSON with `Content-Type: application/problem+json`.
- Internal errors (sqlx, infrastructure) map to `500 internal.error`; the public `detail` is a stable user-safe string; the raw error goes into the tracing log with `error.kind`, `error.chain`.
- `validator`-driven validation errors map to `400 validation.invalid_request`; the response body carries an `errors: { field: [code, ...] }` map.

## 15. Implementation Ordering (informational)

The plan document will sequence implementation. As a rough guide:

1. Repo scaffolding: `Cargo.toml`, empty `lib.rs` / `main.rs`, migrations directory, Dockerfile, compose, CI skeleton.
2. `config`, `db`, `errors`, basic `main.rs` with clap subcommands as no-ops.
3. Migrations 0001–0005 including operator seed + RBAC seed (with audit permissions).
4. `auth` module (JWT, middleware, permission extractors) — *without* permission-denial audit for now.
5. Migration 0006 + `audit` domain: model, persistence, `AuditRecorder` trait, `ChannelAuditRecorder`, `AuditWorker`. Wire a no-op recorder into `AppState` to unblock downstream work.
6. `tenants` persistence + model (roles, organisations, memberships).
7. `security` persistence + model (users, reset tokens).
8. `security` services + handlers: register, login, logout, change-password, switch-org — emit audit events.
9. `tenants` services + handlers: create, add-user, remove-user, list-my, list-members, assign-role — emit audit events.
10. `security` password-reset-request / password-reset-confirm.
11. Extend `auth::permissions::RequirePermission` to emit `permission.denied` audit events.
12. `audit` service + interface: `list-audit-events` handler.
13. `seed-admin` CLI subcommand wiring (with synchronous single-row audit write).
14. OpenAPI annotations + `/docs` + `dump-openapi` subcommand.
15. E2E tests, CI hook for OpenAPI drift.
16. README with quickstart.

## 16. Acceptance — when the seed is "done"

- `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` both clean.
- `cargo test --all-features` green, covering the criteria in §11.4.
- `docker compose up` + `seed-admin` + a curl against `/api/v1/security/login` succeeds end-to-end.
- OpenAPI drift check in CI passes.
- All endpoints in §8.1, §8.2, §8.3, §8.4 are implemented and documented via utoipa.
- Audit worker lifecycle is verified by the `audit_trail` E2E test; shutdown drains the queue.
- A `README.md` in the project root documents quickstart (env vars, migrations, seed-admin, docker-compose).

## Appendix A — Rejected Alternatives

Decisions in this spec were made during an explicit brainstorming session. This appendix records the alternatives that were considered and rejected, and the reasoning. It exists so that future contributors (human or agent) do not re-litigate settled questions without new information, and so that the original trade-offs can be revisited if circumstances change.

### A.1 Operator organisation semantics

**Chosen (§3.5, §5.4):** Super-tenant with elevated privileges — members of `operator` hold `*.manage_all` codes that bypass per-org scoping.

**Rejected:**

- *Regular tenant, just a bootstrap placeholder.* Would simplify the authorisation model (no cross-org bypass rule), but leaves the platform without a first-class administrative surface. Operators would need a separate out-of-band mechanism for cross-tenant support tasks. Revisit only if an external admin tool is built to replace the need.
- *System-only org (no human members).* Cleanly separates "data container for platform things" from "admin actors", but forces a second mechanism for human platform administrators. Doubles the conceptual model.

### A.2 Initial admin seeding

**Chosen (§10.2):** CLI subcommand `egras seed-admin` run deliberately after migrations.

**Rejected:**

- *Auto-seed from env vars on empty DB.* Would be zero-friction for local dev, but couples production startup to secret environment variables and creates a surprising side-effect on first boot. Increases risk of unintentionally promoting a throwaway credential in CI-style environments. Revisit if the seed is ever used under orchestration that *requires* side-effect-free startup.
- *First self-registered user becomes operator admin.* Convenient for self-hosted deployments but opens a window where a public endpoint grants platform-wide privileges; race-sensitive. Rejected as too subtle for a seed meant to be copied into other projects.
- *Manual SQL seed script.* Works but pushes credential handling outside the application boundary, making it easy to seed inconsistent data (e.g., a user without a membership row). CLI subcommand keeps all invariants enforced in one place.

### A.3 Organisation context in JWTs

**Chosen (§6.1, §6.3):** JWT carries an `org` claim; `POST /switch-org` issues a new JWT for a different org.

**Rejected:**

- *Client sends `X-Organisation-Id` header; JWT identifies user only.* Every request would need a membership lookup regardless of cache. Easy for a client to accidentally elevate by supplying the wrong header; defence requires extra middleware. Chosen design instead bakes the scope into the token and validates once at login/switch.
- *JWT lists all memberships; handler requires `org_id` param.* Puts the burden of explicit scoping on every handler, growing request surface and making "what org am I in" ambiguous. Also inflates JWT size for users in many orgs.

### A.4 Membership model

**Chosen (§5.4):** Full RBAC with `roles`, `permissions`, and `role_permissions` tables; `user_organisation_roles` as the membership table.

**Rejected:**

- *Simple membership (no roles).* Minimal schema, but every future permission decision would require a schema migration. Poor fit for a seed meant to be reused.
- *Role per membership (single enum column).* Lighter than full RBAC; sufficient for many apps. Rejected because the target is a seed that teams may extend with app-specific permissions — a permission table means those additions don't touch the schema.
- *Fully dynamic (all roles and permissions editable via API).* Most flexible, significantly more complex surface area (role-CRUD endpoints, invariants on built-ins, authorisation of the authorisation model itself). Disproportionate for an initial seed; the chosen design keeps built-ins as seeded rows and leaves dynamic-role endpoints as an extension.

### A.5 Test file layout

**Chosen (§4, §11.2):** Top-level `tests/` directory that mirrors `src/` one-to-one.

**Rejected:**

- *Sibling `_tests.rs` files inside `src/` via `#[cfg(test)] mod tests;`.* Idiomatic Rust unit tests; allows access to private items. Rejected because it co-locates test code with production code — the spec's constraint was that tests live in separate files, and the chosen layout enforces this at the directory level rather than trusting convention.
- *Separate test crate in a Cargo workspace.* Strongest isolation. Rejected because it forces the production crate into a workspace just for tests and adds friction for day-to-day `cargo test` invocation. If the project later adopts a workspace for other reasons, this becomes attractive again.

### A.6 Crate / workspace structure

**Chosen (§3.1, §4):** Single binary crate (`src/lib.rs` + `src/main.rs`) with a module hierarchy.

**Rejected:**

- *Cargo workspace with one crate per domain.* Enforces domain isolation at the crate level; cleaner dependency graph. Rejected as disproportionate for a seed of this size — compilation times rise, cross-domain shared types need a `core` crate, and workspace-level tooling adds friction. Revisit when the codebase grows large enough that domain builds become a bottleneck.
- *Cargo workspace with one crate per layer (interface/service/model/persistence).* Makes the vertical layering a compile-time boundary. Rejected because cross-domain coupling lives inside each layer crate, inverting the domain model; also unusual in idiomatic Rust and harder for newcomers.

### A.7 RBAC representation

**Chosen (§5.4):** Built-in role seeds + DB-backed permission codes, with role-permission mappings stored in the DB.

**Rejected:**

- *Hardcoded Rust enums for roles and permissions.* Simpler and faster to implement; zero DB schema overhead. Rejected because adding or reassigning a permission requires a code change and deploy, defeating the purpose of RBAC as a configurable control surface.
- *Fully dynamic (all roles and permissions user-editable via API).* See A.4 — rejected for the same reasons of scope/complexity vs. seed goals.

### A.8 Password reset delivery

**Chosen (§6.7):** Log-only stub — token generated and URL logged at INFO.

**Rejected:**

- *Log-only now, SMTP trait abstraction for later.* Introduces a `Mailer` trait and plumbing that is never exercised in the seed's tests. Rejected to keep the seed free of half-finished abstractions. When a project needs email, it can introduce the trait then.
- *Skip password reset entirely in the seed.* Would simplify §6 and §7. Rejected because the flow exercises security primitives (token hashing, TTL, consumed-at marker) that a seed should demonstrate.

### A.9 Layer wiring / dependency injection

**Chosen (§3.2):** Approach A — every service and repository is a `pub` trait; `AppState` holds `Arc<dyn Trait + Send + Sync>`.

**Rejected:**

- *Approach B — generic services with repository trait bounds (`struct RegisterUserService<R: UserRepository>`).* Static dispatch, maximum inlining. Rejected because generic parameters propagate into `AppState` and Axum handler signatures, which then forces either monomorphised router builders or heavy trait objects at the HTTP boundary anyway. The selected test strategy (mock services at interface layer) also requires trait objects. Net effect: complexity without dividend.
- *Approach C — concrete service structs with traits only at persistence boundary.* Straightforward signatures, but interface-layer mocking of services becomes impossible without changing production code. Violates the chosen test strategy (§11.1).

### A.10 Audit storage model

**Chosen (§7.1):** Postgres `audit_events` table as source of truth, mirrored to structured logs.

**Rejected:**

- *Postgres table only.* Halves the observability surface — no log pipeline forwarding without a follow-up change. Chosen design is strictly a superset at negligible cost.
- *Logs only (no DB).* Removes queryability from within the service itself, forces all consumers to integrate with the external log stack, and makes testing audit content significantly harder. Rejected because the spec calls for a first-class query endpoint.

### A.11 Audit write consistency

**Chosen (§7.1, §7.3):** After the business transaction commits, events flow through a bounded mpsc queue drained by a background worker with retry.

**Rejected:**

- *Same transaction as the use case (recommended option at the time).* Strongest consistency — no business change without audit, no orphan audit. Rejected in favour of explicit durability trade-off: audit writes stay off the request critical path, latency under DB pressure is isolated, and the seed demonstrates a worker pattern reusable for other concerns. Revisit if a compliance regime requires in-transaction audit, in which case an outbox pattern (audit row in the same tx with `status=pending`, worker flips to `persisted`) is the upgrade path.
- *Fire-and-forget spawned task per event.* No backpressure; losing a burst of events under load is silent. Rejected in favour of a bounded channel with explicit ERROR on drop.

### A.12 Audit query surface

**Chosen (§7.5, §8.3):** New horizontal domain `audit` with a single `list-audit-events` endpoint.

**Rejected:**

- *No query endpoints (audit written but not queryable via the API).* Simpler surface. Rejected because operator workflows and E2E tests both benefit from direct queryability; pushing this to external log stacks defeats the point of a DB-backed source of truth.
- *Per-domain `list-audit-events` inside `security` and `tenants`.* Keeps the audit domain out of the horizontal set. Rejected because it duplicates query logic, fragments permission checks, and complicates cross-domain queries (e.g., "all events by this user across all domains").

