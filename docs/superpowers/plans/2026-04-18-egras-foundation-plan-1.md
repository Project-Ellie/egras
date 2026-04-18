# egras Foundation + Audit Infrastructure — Implementation Plan (1 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring up a bootable egras server (health/ready/docs only) with config, migrations 0001–0006, JWT + permission auth middleware, and a fully wired audit domain infrastructure (model, persistence, channel recorder, worker, testing harness). No business use cases yet — those come in Plan 2. No permission-denial audit emission yet — that comes in Plan 3.

**Architecture:** Single binary Rust crate (`src/lib.rs` + `src/main.rs`), sqlx+Postgres, Tokio runtime, Axum + tower-http. Auth is a tower Layer that populates request extensions with `Claims` and `PermissionSet`. Audit events flow through a bounded `tokio::sync::mpsc` drained by a background `AuditWorker` with retry.

**Tech Stack:** Rust 2021, Tokio, Axum 0.7.x, tower-http, sqlx 0.8.x, serde, argon2, jsonwebtoken, clap (derive), figment (env), tracing + tracing-subscriber (JSON), uuid v7, chrono, utoipa, testcontainers 0.20.x, mockall, reqwest.

**Reference spec:** `knowledge/specs/2026-04-18-egras-rust-seed-design.md` — treat §1–§7, §9–§14 as authoritative. This plan covers §15 steps 1–5.

---

## File Structure

Files **created** in this plan:

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Crate manifest; runtime + dev deps; `testing` feature |
| `.gitignore` | Standard Rust + `.env` |
| `.env.example` | Every env var from spec §9 |
| `README.md` | Stub — real content in Plan 3 |
| `rust-toolchain.toml` | Pin stable Rust |
| `migrations/0001_extensions.sql` | `citext` extension |
| `migrations/0002_tenants.sql` | `organisations` table |
| `migrations/0003_security.sql` | `users`, `password_reset_tokens` |
| `migrations/0004_rbac.sql` | `roles`, `permissions`, `role_permissions`, `user_organisation_roles` |
| `migrations/0005_seed_operator_and_rbac.sql` | Operator org + built-in roles + permissions + mappings |
| `migrations/0006_audit.sql` | `audit_events` |
| `src/main.rs` | clap dispatch: `serve` \| `seed-admin` \| `dump-openapi` |
| `src/lib.rs` | `pub fn build_app(pool, cfg) -> (Router, AuditWorkerHandle)` + re-exports |
| `src/config.rs` | `AppConfig` loaded from env via figment |
| `src/db.rs` | `PgPool` builder + migration runner |
| `src/errors.rs` | `AppError` + `IntoResponse` → RFC 7807 |
| `src/app_state.rs` | `AppState` struct (audit only for now; grows in Plan 2) |
| `src/testing.rs` | Feature-gated test helpers |
| `src/auth/mod.rs` | Re-exports |
| `src/auth/jwt.rs` | Encode/decode `Claims` |
| `src/auth/permissions.rs` | `PermissionSet`, `RequirePermission` extractor, `AuthorisedOrg` extractor |
| `src/auth/middleware.rs` | `AuthLayer` (tower Layer) |
| `src/audit/mod.rs` | Re-exports |
| `src/audit/model.rs` | `AuditEvent`, `AuditCategory`, `Outcome`, `Actor`, `AuditEventInsert` |
| `src/audit/persistence/mod.rs` | `AuditRepository` trait + re-exports |
| `src/audit/persistence/audit_repository_pg.rs` | sqlx impl |
| `src/audit/service/mod.rs` | Re-exports |
| `src/audit/service/record_event.rs` | `AuditRecorder` trait + `ChannelAuditRecorder` |
| `src/audit/service/list_audit_events.rs` | `ListAuditEvents` trait + impl (handler added in Plan 3) |
| `src/audit/worker.rs` | `AuditWorker` loop, retry, `AuditWorkerHandle` |
| `src/tenants/mod.rs` | Empty placeholder — populated in Plan 2 |
| `src/security/mod.rs` | Empty placeholder — populated in Plan 2 |
| `tests/common/mod.rs`, `tests/common/fixtures.rs`, `tests/common/auth.rs` | Shared test helpers |
| `tests/auth/jwt_test.rs`, `tests/auth/middleware_test.rs`, `tests/auth/permissions_test.rs` | Auth tests |
| `tests/audit/persistence/audit_repository_pg_test.rs` | Persistence test |
| `tests/audit/service/record_event_test.rs`, `tests/audit/service/list_audit_events_test.rs` | Service tests |
| `tests/health_test.rs` | Basic bootable-server E2E |
| `Dockerfile` | Multi-stage build |
| `docker-compose.yml` | Postgres + egras services |
| `.github/workflows/ci.yml` | fmt / clippy / test / build |

No `tests/e2e/` content yet — E2E tests depend on domain use cases (Plan 3).

---

## Task Index

1. Cargo manifest + toolchain
2. `.gitignore` + `.env.example` + stub README
3. Migrations 0001 and 0002 (extensions, tenants)
4. Migrations 0003 and 0004 (security, rbac)
5. Migration 0005 (seed operator + RBAC)
6. Migration 0006 (audit)
7. `errors.rs` — `AppError` enum + RFC 7807 response
8. `errors.rs` — `IntoResponse` impl + unit tests
9. `config.rs` — `AppConfig` + figment loader
10. `db.rs` — `build_pool`, `run_migrations`
11. `main.rs` — clap skeleton, `serve` calls `build_app`
12. `auth/jwt.rs` — `Claims`, `encode_access_token`, `decode_access_token`
13. `auth/permissions.rs` — `PermissionSet` + `RequirePermission` + `AuthorisedOrg`
14. `auth/middleware.rs` — `AuthLayer` that populates extensions
15. `audit/model.rs` — types
16. `audit/persistence/mod.rs` + `audit_repository_pg.rs`
17. `audit/service/record_event.rs` — `AuditRecorder` + `ChannelAuditRecorder`
18. `audit/worker.rs` — worker loop + handle
19. `audit/service/list_audit_events.rs` — service trait + impl
20. `app_state.rs` + `lib.rs::build_app` — health/ready + audit wiring
21. `src/testing.rs` — `TestPool`, `BlockingAuditRecorder`, `mint_jwt`, `MockAppStateBuilder`, `TestApp`
22. `tests/common/*` — shared helpers
23. `tests/health_test.rs` — E2E smoke
24. `Dockerfile` + `docker-compose.yml`
25. `.github/workflows/ci.yml`

---

## Tasks

### Task 1: Cargo manifest + toolchain

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`

- [ ] **Step 1: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2: Create `Cargo.toml`**

```toml
[package]
name = "egras"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
description = "Enterprise-ready Rust application seed (egras)."

[lib]
path = "src/lib.rs"

[[bin]]
name = "egras"
path = "src/main.rs"

[features]
testing = ["dep:testcontainers", "dep:testcontainers-modules"]

[dependencies]
tokio = { version = "1", features = ["full"] }
axum = { version = "0.7", features = ["macros"] }
tower = "0.5"
tower-http = { version = "0.5", features = ["trace", "cors", "request-id", "util"] }
hyper = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "json", "macros", "migrate"] }
validator = { version = "0.18", features = ["derive"] }
argon2 = "0.5"
jsonwebtoken = "9"
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
figment = { version = "0.10", features = ["env", "toml"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }
async-trait = "0.1"
thiserror = "1"
anyhow = "1"
utoipa = { version = "4", features = ["axum_extras", "uuid", "chrono"] }
utoipa-swagger-ui = { version = "7", features = ["axum"] }
rand = "0.8"
sha2 = "0.10"
base64 = "0.22"
hex = "0.4"
testcontainers = { version = "0.20", optional = true }
testcontainers-modules = { version = "0.8", features = ["postgres"], optional = true }

[dev-dependencies]
egras = { path = ".", features = ["testing"] }
mockall = "0.12"
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
tokio-test = "0.4"
pretty_assertions = "1"
tempfile = "3"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles cleanly (empty library — we'll add `src/lib.rs` next but cargo tolerates the missing target briefly). If `cargo check` complains about missing `src/lib.rs`, create an empty one first: `touch src/lib.rs`.

- [ ] **Step 4: Commit**

```bash
mkdir -p src
: > src/lib.rs
git add Cargo.toml rust-toolchain.toml src/lib.rs
git commit -m "chore: initial Cargo manifest and toolchain pin"
```

---

### Task 2: `.gitignore`, `.env.example`, stub README

**Files:**
- Create: `.gitignore`
- Create: `.env.example`
- Create: `README.md`

- [ ] **Step 1: Create `.gitignore`**

```gitignore
/target
Cargo.lock.bak
.env
.env.local
*.swp
.DS_Store
```

Note: `Cargo.lock` is **committed** for binary crates. Do not ignore it.

- [ ] **Step 2: Create `.env.example`** (lists every env var from spec §9)

```dotenv
# Postgres
EGRAS_DATABASE_URL=postgres://egras:egras@localhost:5432/egras
EGRAS_DATABASE_MAX_CONNECTIONS=10

# HTTP
EGRAS_BIND_ADDRESS=0.0.0.0:8080

# JWT (generate with: openssl rand -hex 32)
EGRAS_JWT_SECRET=replace-me-with-32-bytes-of-entropy-xxxxx
EGRAS_JWT_TTL_SECS=3600
EGRAS_JWT_ISSUER=egras

# Logging
EGRAS_LOG_LEVEL=info
EGRAS_LOG_FORMAT=json

# CORS
EGRAS_CORS_ALLOWED_ORIGINS=

# Password reset
EGRAS_PASSWORD_RESET_TTL_SECS=3600

# Operator bootstrap
EGRAS_OPERATOR_ORG_NAME=operator

# Audit worker
EGRAS_AUDIT_CHANNEL_CAPACITY=4096
EGRAS_AUDIT_MAX_RETRIES=3
EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL=100
```

- [ ] **Step 3: Create `README.md`** (stub; real content lands in Plan 3)

```markdown
# egras

Enterprise-ready Rust application seed. See `knowledge/specs/2026-04-18-egras-rust-seed-design.md` for the authoritative design.

Quickstart docs arrive with the completion of Plan 3.
```

- [ ] **Step 4: Commit**

```bash
git add .gitignore .env.example README.md
git commit -m "chore: add gitignore, env example, and stub README"
```

---

### Task 3: Migrations 0001 (extensions) and 0002 (tenants)

**Files:**
- Create: `migrations/0001_extensions.sql`
- Create: `migrations/0002_tenants.sql`

- [ ] **Step 1: Create `migrations/0001_extensions.sql`**

```sql
CREATE EXTENSION IF NOT EXISTS citext;
```

- [ ] **Step 2: Create `migrations/0002_tenants.sql`**

```sql
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

- [ ] **Step 3: Sanity — apply to a scratch DB**

Run (requires local Postgres or `docker run --rm -e POSTGRES_PASSWORD=pw -p 5433:5432 postgres:16-alpine` in another terminal):

```bash
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -c "CREATE DATABASE egras_scratch;"
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -f migrations/0001_extensions.sql
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -f migrations/0002_tenants.sql
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -c "\d organisations"
```
Expected: table lists the columns above; `ux_organisations_operator` partial unique index exists.

- [ ] **Step 4: Commit**

```bash
git add migrations/0001_extensions.sql migrations/0002_tenants.sql
git commit -m "feat(migrations): add 0001 extensions and 0002 tenants"
```

---

### Task 4: Migrations 0003 (security) and 0004 (rbac)

**Files:**
- Create: `migrations/0003_security.sql`
- Create: `migrations/0004_rbac.sql`

- [ ] **Step 1: Create `migrations/0003_security.sql`**

```sql
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE,
    email           CITEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE password_reset_tokens (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_prt_user ON password_reset_tokens (user_id);
```

- [ ] **Step 2: Create `migrations/0004_rbac.sql`**

```sql
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

- [ ] **Step 3: Apply to scratch DB as in Task 3 Step 3, then `\d users`, `\d roles`, etc.**
Expected: all tables and indexes exist; FKs resolve.

- [ ] **Step 4: Commit**

```bash
git add migrations/0003_security.sql migrations/0004_rbac.sql
git commit -m "feat(migrations): add 0003 security and 0004 rbac"
```

---

### Task 5: Migration 0005 — seed operator org + RBAC

**Files:**
- Create: `migrations/0005_seed_operator_and_rbac.sql`

Deterministic UUIDs are used so tests and the bootstrap check (§10.1 step 4) can reference them by name. Use UUIDv4-shaped constant strings; since they're seeds, stability > v7 ordering.

- [ ] **Step 1: Create `migrations/0005_seed_operator_and_rbac.sql`**

```sql
-- Operator organisation (deterministic UUID, per spec §5.3)
INSERT INTO organisations (id, name, business, is_operator)
VALUES ('00000000-0000-0000-0000-000000000001', 'operator', 'Platform Operator', TRUE)
ON CONFLICT (id) DO NOTHING;

-- Built-in roles (deterministic UUIDs so tests can reference)
INSERT INTO roles (id, code, name, description, is_builtin) VALUES
  ('00000000-0000-0000-0000-000000000101', 'operator_admin', 'Operator Admin', 'Platform-wide administrator', TRUE),
  ('00000000-0000-0000-0000-000000000102', 'org_owner',      'Organisation Owner', 'Owns a tenant organisation', TRUE),
  ('00000000-0000-0000-0000-000000000103', 'org_admin',      'Organisation Admin', 'Manages a tenant organisation', TRUE),
  ('00000000-0000-0000-0000-000000000104', 'org_member',     'Organisation Member', 'Member of a tenant organisation', TRUE)
ON CONFLICT (id) DO NOTHING;

-- Permissions (UUIDv4-ish deterministic)
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000201', 'tenants.manage_all',      'Operate on any tenant, bypassing org scope'),
  ('00000000-0000-0000-0000-000000000202', 'users.manage_all',        'Manage any user account'),
  ('00000000-0000-0000-0000-000000000203', 'tenants.create',          'Create a new organisation'),
  ('00000000-0000-0000-0000-000000000204', 'tenants.update',          'Update an organisation the caller owns'),
  ('00000000-0000-0000-0000-000000000205', 'tenants.read',            'Read organisation metadata'),
  ('00000000-0000-0000-0000-000000000206', 'tenants.members.add',     'Add a user to an organisation'),
  ('00000000-0000-0000-0000-000000000207', 'tenants.members.remove',  'Remove a user from an organisation'),
  ('00000000-0000-0000-0000-000000000208', 'tenants.members.list',    'List members of an organisation'),
  ('00000000-0000-0000-0000-000000000209', 'tenants.roles.assign',    'Assign a role to a user in an organisation'),
  ('00000000-0000-0000-0000-00000000020a', 'audit.read_all',          'Read audit events across all organisations'),
  ('00000000-0000-0000-0000-00000000020b', 'audit.read_own_org',      'Read audit events for own organisation')
ON CONFLICT (id) DO NOTHING;

-- Role → permission mappings (spec §5.4)
-- operator_admin: everything
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'operator_admin'
  AND p.code IN (
    'tenants.manage_all', 'users.manage_all',
    'tenants.create', 'tenants.update', 'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_all', 'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_owner
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_owner'
  AND p.code IN (
    'tenants.create', 'tenants.update', 'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_admin
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_admin'
  AND p.code IN (
    'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_member
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_member'
  AND p.code IN (
    'tenants.read',
    'tenants.members.list'
  )
ON CONFLICT DO NOTHING;
```

- [ ] **Step 2: Apply to scratch DB and verify matrix**

Run:

```bash
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch \
  -f migrations/0005_seed_operator_and_rbac.sql

PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -c "
SELECT r.code, count(p.code)
FROM roles r LEFT JOIN role_permissions rp ON rp.role_id = r.id
LEFT JOIN permissions p ON p.id = rp.permission_id
GROUP BY r.code ORDER BY r.code;"
```

Expected counts — `operator_admin`: 11, `org_owner`: 8, `org_admin`: 6, `org_member`: 2. Matches the matrix in spec §5.4.

- [ ] **Step 3: Commit**

```bash
git add migrations/0005_seed_operator_and_rbac.sql
git commit -m "feat(migrations): seed operator org, built-in roles, permissions, mappings"
```

---

### Task 6: Migration 0006 — audit

**Files:**
- Create: `migrations/0006_audit.sql`

- [ ] **Step 1: Create `migrations/0006_audit.sql`**

```sql
CREATE TABLE audit_events (
    id                       UUID PRIMARY KEY,
    occurred_at              TIMESTAMPTZ NOT NULL,
    recorded_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    category                 TEXT NOT NULL,
    event_type               TEXT NOT NULL,
    actor_user_id            UUID,
    actor_organisation_id    UUID,
    target_type              TEXT,
    target_id                UUID,
    target_organisation_id   UUID,
    request_id               TEXT,
    ip_address               INET,
    user_agent               TEXT,
    outcome                  TEXT NOT NULL,
    reason_code              TEXT,
    payload                  JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX ix_audit_occurred_at   ON audit_events (occurred_at DESC);
CREATE INDEX ix_audit_target_org    ON audit_events (target_organisation_id, occurred_at DESC);
CREATE INDEX ix_audit_actor         ON audit_events (actor_user_id, occurred_at DESC);
CREATE INDEX ix_audit_event_type    ON audit_events (event_type, occurred_at DESC);
```

- [ ] **Step 2: Apply to scratch DB**

```bash
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -f migrations/0006_audit.sql
PGPASSWORD=pw psql -h localhost -p 5433 -U postgres -d egras_scratch -c "\d audit_events"
```
Expected: all columns and indexes present.

- [ ] **Step 3: Commit**

```bash
git add migrations/0006_audit.sql
git commit -m "feat(migrations): add 0006 audit_events table and indexes"
```

---

### Task 7: `errors.rs` — `AppError` enum

**Files:**
- Create: `src/errors.rs`
- Modify: `src/lib.rs` (add `pub mod errors;`)

- [ ] **Step 1: Write the failing test**

Create `tests/errors_test.rs`:

```rust
use egras::errors::{AppError, ErrorSlug};

#[test]
fn slug_of_permission_denied_is_stable() {
    let err = AppError::PermissionDenied { code: "tenants.members.add".into() };
    assert_eq!(err.slug(), ErrorSlug::PermissionDenied);
    assert_eq!(err.http_status().as_u16(), 403);
}

#[test]
fn slug_of_validation_is_400() {
    let err = AppError::Validation { errors: Default::default() };
    assert_eq!(err.http_status().as_u16(), 400);
}

#[test]
fn slug_of_unauthenticated_is_401() {
    let err = AppError::Unauthenticated { reason: "missing_header".into() };
    assert_eq!(err.http_status().as_u16(), 401);
}

#[test]
fn slug_of_not_found_is_404() {
    let err = AppError::NotFound { resource: "organisation".into() };
    assert_eq!(err.http_status().as_u16(), 404);
}

#[test]
fn slug_of_conflict_is_409() {
    let err = AppError::Conflict { reason: "last_owner".into() };
    assert_eq!(err.http_status().as_u16(), 409);
}

#[test]
fn internal_error_is_500() {
    let err = AppError::Internal(anyhow::anyhow!("boom"));
    assert_eq!(err.http_status().as_u16(), 500);
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --test errors_test`
Expected: compilation error, `AppError` not found.

- [ ] **Step 3: Write `src/errors.rs`**

```rust
use std::collections::HashMap;

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;
use thiserror::Error;

/// Canonical error slugs from spec §8.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorSlug {
    #[serde(rename = "validation.invalid_request")]
    ValidationInvalidRequest,
    #[serde(rename = "auth.unauthenticated")]
    AuthUnauthenticated,
    #[serde(rename = "auth.invalid_credentials")]
    AuthInvalidCredentials,
    #[serde(rename = "permission.denied")]
    PermissionDenied,
    #[serde(rename = "resource.not_found")]
    ResourceNotFound,
    #[serde(rename = "resource.conflict")]
    ResourceConflict,
    #[serde(rename = "user.no_organisation")]
    UserNoOrganisation,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "internal.error")]
    InternalError,
}

impl ErrorSlug {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ValidationInvalidRequest => "validation.invalid_request",
            Self::AuthUnauthenticated => "auth.unauthenticated",
            Self::AuthInvalidCredentials => "auth.invalid_credentials",
            Self::PermissionDenied => "permission.denied",
            Self::ResourceNotFound => "resource.not_found",
            Self::ResourceConflict => "resource.conflict",
            Self::UserNoOrganisation => "user.no_organisation",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal.error",
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation failed")]
    Validation { errors: HashMap<String, Vec<String>> },

    #[error("unauthenticated: {reason}")]
    Unauthenticated { reason: String },

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("permission denied: missing {code}")]
    PermissionDenied { code: String },

    #[error("not found: {resource}")]
    NotFound { resource: String },

    #[error("conflict: {reason}")]
    Conflict { reason: String },

    #[error("user has no organisation")]
    UserNoOrganisation,

    #[error("rate limited")]
    RateLimited,

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn slug(&self) -> ErrorSlug {
        match self {
            Self::Validation { .. } => ErrorSlug::ValidationInvalidRequest,
            Self::Unauthenticated { .. } => ErrorSlug::AuthUnauthenticated,
            Self::InvalidCredentials => ErrorSlug::AuthInvalidCredentials,
            Self::PermissionDenied { .. } => ErrorSlug::PermissionDenied,
            Self::NotFound { .. } => ErrorSlug::ResourceNotFound,
            Self::Conflict { .. } => ErrorSlug::ResourceConflict,
            Self::UserNoOrganisation => ErrorSlug::UserNoOrganisation,
            Self::RateLimited => ErrorSlug::RateLimited,
            Self::Internal(_) => ErrorSlug::InternalError,
        }
    }

    pub fn http_status(&self) -> StatusCode {
        match self {
            Self::Validation { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::PermissionDenied { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::UserNoOrganisation => StatusCode::FORBIDDEN,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.slug() {
            ErrorSlug::ValidationInvalidRequest => "Invalid request",
            ErrorSlug::AuthUnauthenticated => "Unauthenticated",
            ErrorSlug::AuthInvalidCredentials => "Invalid credentials",
            ErrorSlug::PermissionDenied => "Permission denied",
            ErrorSlug::ResourceNotFound => "Not found",
            ErrorSlug::ResourceConflict => "Conflict",
            ErrorSlug::UserNoOrganisation => "User has no organisation",
            ErrorSlug::RateLimited => "Rate limited",
            ErrorSlug::InternalError => "Internal error",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Validation { .. } => "One or more fields failed validation.".to_string(),
            Self::Unauthenticated { reason } => format!("Authentication required ({reason})."),
            Self::InvalidCredentials => "Invalid username or password.".to_string(),
            Self::PermissionDenied { code } => format!("missing permission: {code}"),
            Self::NotFound { resource } => format!("{resource} was not found."),
            Self::Conflict { reason } => reason.clone(),
            Self::UserNoOrganisation => "The user does not belong to any organisation.".to_string(),
            Self::RateLimited => "Too many requests; retry later.".to_string(),
            Self::Internal(_) => "An internal error occurred.".to_string(),
        }
    }
}
```

*Note:* `IntoResponse` is added in Task 8 alongside the RFC 7807 body test.

- [ ] **Step 4: Modify `src/lib.rs` to expose the module**

```rust
pub mod errors;
```

- [ ] **Step 5: Run the test**

Run: `cargo test --test errors_test`
Expected: all 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/errors.rs src/lib.rs tests/errors_test.rs
git commit -m "feat(errors): AppError enum with canonical slug mapping"
```

---

### Task 8: `IntoResponse` for `AppError` (RFC 7807)

**Files:**
- Modify: `src/errors.rs`
- Modify: `tests/errors_test.rs`

- [ ] **Step 1: Append a failing test**

Append to `tests/errors_test.rs`:

```rust
use axum::response::IntoResponse;
use axum::body::to_bytes;
use serde_json::Value;

#[tokio::test]
async fn permission_denied_serialises_as_rfc7807() {
    let err = AppError::PermissionDenied { code: "tenants.members.add".into() };
    let resp = err.into_response();
    assert_eq!(resp.status().as_u16(), 403);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.starts_with("application/problem+json"));

    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["type"], "https://egras.dev/errors/permission.denied");
    assert_eq!(v["title"], "Permission denied");
    assert_eq!(v["status"], 403);
    assert_eq!(v["detail"], "missing permission: tenants.members.add");
}

#[tokio::test]
async fn validation_error_includes_errors_map() {
    let mut errors = std::collections::HashMap::new();
    errors.insert("email".to_string(), vec!["format".to_string()]);
    let err = AppError::Validation { errors };
    let resp = err.into_response();
    assert_eq!(resp.status().as_u16(), 400);
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["type"], "https://egras.dev/errors/validation.invalid_request");
    assert_eq!(v["errors"]["email"][0], "format");
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --test errors_test`
Expected: `into_response` not implemented — compilation or linker error.

- [ ] **Step 3: Append to `src/errors.rs`**

```rust
const TYPE_PREFIX: &str = "https://egras.dev/errors/";

#[derive(Debug, Serialize)]
struct ProblemJson<'a> {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'a str,
    status: u16,
    detail: String,
    instance: Option<&'a str>,
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a HashMap<String, Vec<String>>>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.http_status();
        let slug = self.slug();
        let title = self.title();
        let detail = self.detail();
        let errors_ref = if let AppError::Validation { errors } = &self { Some(errors) } else { None };

        // Log internal errors with full chain before we drop the error.
        if let AppError::Internal(err) = &self {
            tracing::error!(error.kind = "internal", error.chain = %err, "internal error");
        }

        let body = ProblemJson {
            type_uri: format!("{TYPE_PREFIX}{}", slug.as_str()),
            title,
            status: status.as_u16(),
            detail,
            instance: None,
            request_id: None, // populated by a downstream layer if desired
            errors: errors_ref,
        };

        let mut resp = (status, Json(body)).into_response();
        resp.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        resp
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test errors_test`
Expected: all 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/errors.rs tests/errors_test.rs
git commit -m "feat(errors): RFC 7807 IntoResponse for AppError"
```

---

### Task 9: `config.rs` — `AppConfig` + figment loader

**Files:**
- Create: `src/config.rs`
- Create: `tests/config_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/config_test.rs`:

```rust
use egras::config::AppConfig;

fn set_required_env() {
    std::env::set_var("EGRAS_DATABASE_URL", "postgres://e:e@localhost/e");
    std::env::set_var("EGRAS_JWT_SECRET", "a".repeat(64));
}

#[test]
fn loads_with_defaults() {
    set_required_env();
    let cfg = AppConfig::from_env().expect("config loads");
    assert_eq!(cfg.bind_address, "0.0.0.0:8080");
    assert_eq!(cfg.jwt_ttl_secs, 3600);
    assert_eq!(cfg.audit_channel_capacity, 4096);
    assert_eq!(cfg.audit_max_retries, 3);
}

#[test]
fn rejects_short_jwt_secret() {
    std::env::set_var("EGRAS_DATABASE_URL", "postgres://e:e@localhost/e");
    std::env::set_var("EGRAS_JWT_SECRET", "short");
    let err = AppConfig::from_env().expect_err("must reject short secret");
    assert!(format!("{err:#}").contains("EGRAS_JWT_SECRET"));
}
```

*Note:* these tests set process-wide env; keep this test file single-threaded (default for separate `tests/*.rs` files — each integration test file gets its own binary). Do not add a second test file that mutates the same env.

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --test config_test`
Expected: compilation error — `AppConfig::from_env` not defined.

- [ ] **Step 3: Write `src/config.rs`**

```rust
use figment::{providers::Env, Figment};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    #[serde(default = "default_db_max")]
    pub database_max_connections: u32,
    #[serde(default = "default_bind")]
    pub bind_address: String,
    pub jwt_secret: String,
    #[serde(default = "default_jwt_ttl")]
    pub jwt_ttl_secs: i64,
    #[serde(default = "default_jwt_iss")]
    pub jwt_issuer: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub cors_allowed_origins: String,
    #[serde(default = "default_reset_ttl")]
    pub password_reset_ttl_secs: i64,
    #[serde(default = "default_operator_name")]
    pub operator_org_name: String,
    #[serde(default = "default_audit_capacity")]
    pub audit_channel_capacity: usize,
    #[serde(default = "default_audit_retries")]
    pub audit_max_retries: u32,
    #[serde(default = "default_audit_backoff")]
    pub audit_retry_backoff_ms_initial: u64,
}

fn default_db_max() -> u32 { 10 }
fn default_bind() -> String { "0.0.0.0:8080".into() }
fn default_jwt_ttl() -> i64 { 3600 }
fn default_jwt_iss() -> String { "egras".into() }
fn default_log_level() -> String { "info".into() }
fn default_log_format() -> String { "json".into() }
fn default_reset_ttl() -> i64 { 3600 }
fn default_operator_name() -> String { "operator".into() }
fn default_audit_capacity() -> usize { 4096 }
fn default_audit_retries() -> u32 { 3 }
fn default_audit_backoff() -> u64 { 100 }

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let cfg: AppConfig = Figment::new()
            .merge(Env::prefixed("EGRAS_"))
            .extract()
            .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 32 {
            anyhow::bail!("EGRAS_JWT_SECRET must be at least 32 bytes (got {})", self.jwt_secret.len());
        }
        if !["json", "pretty"].contains(&self.log_format.as_str()) {
            anyhow::bail!("EGRAS_LOG_FORMAT must be 'json' or 'pretty' (got {})", self.log_format);
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Modify `src/lib.rs`**

```rust
pub mod config;
pub mod errors;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test config_test`
Expected: both tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/lib.rs tests/config_test.rs
git commit -m "feat(config): AppConfig with figment env loader and validation"
```

---

### Task 10: `db.rs` — pool builder + migration runner

**Files:**
- Create: `src/db.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write `src/db.rs`**

Unit-testing this in isolation would require a live Postgres; we rely on the E2E smoke test (Task 23) and downstream tests to exercise it. So this task is implementation + clippy-clean, no dedicated unit test.

```rust
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::config::AppConfig;

pub async fn build_pool(cfg: &AppConfig) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.database_max_connections)
        .connect(&cfg.database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
```

- [ ] **Step 2: Modify `src/lib.rs`**

```rust
pub mod config;
pub mod db;
pub mod errors;
```

- [ ] **Step 3: Verify clippy clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add src/db.rs src/lib.rs
git commit -m "feat(db): pool builder and migration runner"
```

---

### Task 11: `main.rs` — clap CLI with `serve` stub

**Files:**
- Create: `src/main.rs`

`seed-admin` and `dump-openapi` are declared but return "not implemented yet" with exit 2 — they land fully in Plan 3. `serve` is wired at this point but `build_app` is still a stub until Task 20.

- [ ] **Step 1: Write `src/main.rs`**

```rust
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use egras::config::AppConfig;

#[derive(Parser)]
#[command(name = "egras", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the HTTP server (default).
    Serve,
    /// Seed the first operator admin user.
    SeedAdmin {
        #[arg(long)] email: String,
        #[arg(long)] username: String,
        #[arg(long)] password: String,
        #[arg(long, default_value = "operator_admin")] role: String,
    },
    /// Dump OpenAPI 3.1 JSON to stdout.
    DumpOpenapi,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = AppConfig::from_env()?;
    init_tracing(&cfg);

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => run_serve(cfg).await,
        Commands::SeedAdmin { .. } => {
            eprintln!("seed-admin: not implemented yet (Plan 3)");
            std::process::exit(2);
        }
        Commands::DumpOpenapi => {
            eprintln!("dump-openapi: not implemented yet (Plan 3)");
            std::process::exit(2);
        }
    }
}

fn init_tracing(cfg: &AppConfig) {
    let filter = EnvFilter::try_new(&cfg.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    if cfg.log_format == "json" {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer().pretty()).init();
    }
}

async fn run_serve(cfg: AppConfig) -> anyhow::Result<()> {
    let pool = egras::db::build_pool(&cfg).await?;
    egras::db::run_migrations(&pool).await?;

    let (router, audit_handle) = egras::build_app(pool.clone(), cfg.clone()).await?;

    let listener = tokio::net::TcpListener::bind(&cfg.bind_address).await?;
    tracing::info!(bind = %cfg.bind_address, "egras listening");

    let shutdown = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c().await.ok();
        };
        #[cfg(unix)]
        let term = async {
            let mut s = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate(),
            ).expect("install SIGTERM handler");
            s.recv().await;
        };
        #[cfg(not(unix))]
        let term = std::future::pending::<()>();

        tokio::select! { _ = ctrl_c => {}, _ = term => {} }
        tracing::info!("shutdown signal received");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;

    audit_handle.shutdown().await;
    pool.close().await;
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles (build_app is stubbed in Task 20 — for now we expect a missing-function error)**

Run: `cargo build`
Expected: unresolved reference `egras::build_app`. This is intentional — we'll define it in Task 20. We skip the commit until Task 20 binds `build_app`. Instead, proceed to next tasks and come back to commit `main.rs` after Task 20.

- [ ] **Step 3: Stash for now**

Do not commit yet. Leave `src/main.rs` on disk. The final commit happens in Task 20 alongside `lib.rs::build_app`.

---

### Task 12: `auth/jwt.rs` — `Claims`, encode, decode

**Files:**
- Create: `src/auth/mod.rs`
- Create: `src/auth/jwt.rs`
- Create: `tests/auth/jwt_test.rs` (as `tests/auth_jwt_test.rs` because Rust integration tests ignore subdirectories unless declared as modules — see note)
- Modify: `src/lib.rs`

*Test layout note:* Cargo treats every file directly under `tests/` as its own integration test binary. Files in subdirectories under `tests/` are NOT automatically compiled; they must be referenced via `mod` from a sibling `.rs` file. The spec's §4 test tree mirrors `src/` using subdirectories; to make Cargo pick those up, each `tests/<domain>/<layer>/<name>_test.rs` file is re-exported from a top-level `tests/<domain>_<layer>_<name>_test.rs` stub that just does `#[path = "<domain>/<layer>/<name>_test.rs"] mod it;`, OR the spec's intent is satisfied by flat filenames such as `tests/auth_jwt_test.rs`. We adopt **flat filenames** for simplicity; the `tests/` tree remains organised by naming prefix. (The spec's §4 filesystem layout is illustrative of logical grouping.)

- [ ] **Step 1: Write the failing test**

Create `tests/auth_jwt_test.rs`:

```rust
use egras::auth::jwt::{encode_access_token, decode_access_token, Claims};
use uuid::Uuid;

#[test]
fn encode_then_decode_roundtrip() {
    let secret = "a".repeat(64);
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&secret, "egras", sub, org, 3600).unwrap();

    let claims: Claims = decode_access_token(&secret, "egras", &token).unwrap();
    assert_eq!(claims.sub, sub);
    assert_eq!(claims.org, org);
    assert_eq!(claims.iss, "egras");
    assert_eq!(claims.typ, "access");
    assert!(claims.exp > claims.iat);
}

#[test]
fn rejects_bad_signature() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, 3600).unwrap();
    let err = decode_access_token(&"b".repeat(64), "egras", &token).expect_err("bad sig");
    let s = format!("{err:#}");
    assert!(s.contains("signature") || s.contains("Invalid"), "got: {s}");
}

#[test]
fn rejects_wrong_issuer() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, 3600).unwrap();
    assert!(decode_access_token(&"a".repeat(64), "nope", &token).is_err());
}

#[test]
fn rejects_expired_token() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    // ttl = -10 means already expired
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, -10).unwrap();
    assert!(decode_access_token(&"a".repeat(64), "egras", &token).is_err());
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --test auth_jwt_test`
Expected: compile error — `egras::auth::jwt` missing.

- [ ] **Step 3: Create `src/auth/mod.rs`**

```rust
pub mod jwt;
pub mod permissions;
pub mod middleware;
```

- [ ] **Step 4: Create `src/auth/jwt.rs`**

```rust
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub org: Uuid,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
    pub iss: String,
    pub typ: String,
}

pub fn encode_access_token(
    secret: &str,
    issuer: &str,
    user_id: Uuid,
    org_id: Uuid,
    ttl_secs: i64,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        org: org_id,
        iat: now,
        exp: now + ttl_secs,
        jti: Uuid::now_v7(),
        iss: issuer.to_string(),
        typ: "access".to_string(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_access_token(
    secret: &str,
    expected_issuer: &str,
    token: &str,
) -> anyhow::Result<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[expected_issuer]);
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    if data.claims.typ != "access" {
        anyhow::bail!("token typ is not 'access'");
    }
    Ok(data.claims)
}
```

- [ ] **Step 5: Update `src/lib.rs`**

```rust
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test auth_jwt_test`
Expected: all 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/auth tests/auth_jwt_test.rs src/lib.rs
git commit -m "feat(auth): JWT encode/decode with HS256 and validation"
```

---

### Task 13: `auth/permissions.rs` — `PermissionSet`, `RequirePermission`, `AuthorisedOrg`

**Files:**
- Create: `src/auth/permissions.rs`
- Create: `tests/auth_permissions_test.rs`

Permission-denial audit emission is **deferred to Plan 3** (§15 step 11). The extractor here returns `AppError::PermissionDenied` only.

- [ ] **Step 1: Write the failing tests**

Create `tests/auth_permissions_test.rs`:

```rust
use egras::auth::permissions::PermissionSet;

#[test]
fn permission_set_matches_exact_code() {
    let s = PermissionSet::from_codes(vec!["tenants.read".into(), "tenants.members.list".into()]);
    assert!(s.has("tenants.read"));
    assert!(s.has("tenants.members.list"));
    assert!(!s.has("tenants.members.add"));
}

#[test]
fn permission_set_operator_flags() {
    let s = PermissionSet::from_codes(vec!["tenants.manage_all".into()]);
    assert!(s.is_operator_over_tenants());
    assert!(!s.is_audit_read_all());

    let s2 = PermissionSet::from_codes(vec!["audit.read_all".into()]);
    assert!(s2.is_audit_read_all());
}

#[test]
fn permission_set_any_match() {
    let s = PermissionSet::from_codes(vec!["tenants.members.add".into()]);
    assert!(s.has_any(&["users.manage_all", "tenants.members.add"]));
    assert!(!s.has_any(&["users.manage_all", "tenants.roles.assign"]));
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --test auth_permissions_test`
Expected: compilation error — module missing.

- [ ] **Step 3: Write `src/auth/permissions.rs`**

```rust
use std::collections::HashSet;

use async_trait::async_trait;
use axum::{extract::FromRequestParts, http::request::Parts};
use uuid::Uuid;

use crate::auth::jwt::Claims;
use crate::errors::AppError;

#[derive(Debug, Clone, Default)]
pub struct PermissionSet {
    codes: HashSet<String>,
}

impl PermissionSet {
    pub fn from_codes<I: IntoIterator<Item = String>>(codes: I) -> Self {
        Self { codes: codes.into_iter().collect() }
    }

    pub fn has(&self, code: &str) -> bool {
        self.codes.contains(code)
    }

    pub fn has_any(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.codes.contains(*c))
    }

    pub fn is_operator_over_tenants(&self) -> bool {
        self.has("tenants.manage_all")
    }

    pub fn is_operator_over_users(&self) -> bool {
        self.has("users.manage_all")
    }

    pub fn is_audit_read_all(&self) -> bool {
        self.has("audit.read_all")
    }

    /// Iterate codes (deterministic order for logging/testing).
    pub fn iter_sorted(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.codes.iter().map(String::as_str).collect();
        v.sort();
        v
    }
}

/// Axum extractor that enforces the caller holds `code`.
pub struct RequirePermission(pub &'static str);

#[async_trait]
impl<S> FromRequestParts<S> for RequirePermission
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // This is a marker extractor; real usage is `require(parts, "code")` in handlers,
        // but we preserve the struct form for ergonomics. See `require_permission`.
        let _ = parts;
        // Caller should never instantiate without specifying the code; this branch
        // intentionally rejects the marker usage to catch misuse.
        Err(AppError::Internal(anyhow::anyhow!(
            "RequirePermission must not be used as a bare extractor; call `require_permission`."
        )))
    }
}

/// Call from a handler: `require_permission(&parts, "tenants.members.add")?;`
pub fn require_permission(parts: &Parts, code: &'static str) -> Result<(), AppError> {
    let set = parts.extensions.get::<PermissionSet>().ok_or_else(|| {
        AppError::Unauthenticated { reason: "no_permission_set".into() }
    })?;
    if set.has(code) {
        Ok(())
    } else {
        Err(AppError::PermissionDenied { code: code.to_string() })
    }
}

/// Extract the caller's JWT `org_id` and enforce the cross-org rule from spec §3.5.
///
/// If the caller has `*.manage_all` or `audit.read_all`, they may operate on any org;
/// otherwise mismatched `organisation_id` → 404 (via `AppError::NotFound`).
pub fn authorise_org(parts: &Parts, organisation_id: Uuid) -> Result<(), AppError> {
    let claims = parts.extensions.get::<Claims>().ok_or_else(|| {
        AppError::Unauthenticated { reason: "no_claims".into() }
    })?;
    let set = parts.extensions.get::<PermissionSet>().ok_or_else(|| {
        AppError::Unauthenticated { reason: "no_permission_set".into() }
    })?;
    if set.is_operator_over_tenants() || set.is_operator_over_users() || set.is_audit_read_all() {
        return Ok(());
    }
    if claims.org == organisation_id {
        Ok(())
    } else {
        Err(AppError::NotFound { resource: "organisation".into() })
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test auth_permissions_test`
Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/auth/permissions.rs tests/auth_permissions_test.rs
git commit -m "feat(auth): PermissionSet and require_permission/authorise_org helpers"
```

---

### Task 14: `auth/middleware.rs` — `AuthLayer`

**Files:**
- Create: `src/auth/middleware.rs`
- Create: `tests/auth_middleware_test.rs`

The middleware:
1. Extracts `Authorization: Bearer <jwt>`.
2. Decodes + validates (iss, exp, typ) via `auth::jwt::decode_access_token`.
3. Loads permission codes for `(user_id, org_id)` from the DB by joining `user_organisation_roles → role_permissions → permissions`.
4. Inserts `Claims` and `PermissionSet` into request extensions.

The permission-loading SQL runs on every request (no cache in this seed; fine for seed scope — a cache is an explicit follow-up).

- [ ] **Step 1: Write the failing test**

Create `tests/auth_middleware_test.rs`:

```rust
use axum::{body::Body, http::{Request, StatusCode}, Router, routing::get};
use egras::auth::middleware::AuthLayer;
use egras::auth::jwt::{encode_access_token, Claims};
use egras::auth::permissions::PermissionSet;
use tower::ServiceExt;
use uuid::Uuid;

async fn echo_handler(
    axum::Extension(claims): axum::Extension<Claims>,
    axum::Extension(perms): axum::Extension<PermissionSet>,
) -> String {
    format!("{} {:?}", claims.sub, perms.iter_sorted())
}

fn router_with_static_permissions() -> Router {
    // For unit tests of the middleware we provide a "static" permission loader
    // that returns a fixed set regardless of user/org. The real loader is
    // exercised by the integration test on a real DB (Task 23+ in later plans).
    let secret = "a".repeat(64);
    let loader = egras::auth::middleware::PermissionLoader::static_codes(vec![
        "tenants.read".into(),
        "tenants.members.list".into(),
    ]);
    Router::new()
        .route("/echo", get(echo_handler))
        .layer(AuthLayer::new(secret.clone(), "egras".into(), loader))
}

#[tokio::test]
async fn rejects_missing_authorization_header() {
    let app = router_with_static_permissions();
    let resp = app
        .oneshot(Request::builder().uri("/echo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_bad_token() {
    let app = router_with_static_permissions();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/echo")
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn accepts_valid_token_and_injects_extensions() {
    let app = router_with_static_permissions();
    let secret = "a".repeat(64);
    let token = encode_access_token(&secret, "egras", Uuid::now_v7(), Uuid::now_v7(), 3600).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/echo")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let s = String::from_utf8(body.to_vec()).unwrap();
    assert!(s.contains("tenants.read"));
    assert!(s.contains("tenants.members.list"));
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --test auth_middleware_test`
Expected: compilation error.

- [ ] **Step 3: Write `src/auth/middleware.rs`**

```rust
use std::{future::Future, pin::Pin, sync::Arc, task::{Context, Poll}};

use async_trait::async_trait;
use axum::{body::Body, http::{Request, Response, StatusCode, header}, response::IntoResponse};
use sqlx::PgPool;
use tower::{Layer, Service};
use uuid::Uuid;

use crate::auth::jwt::{decode_access_token, Claims};
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

/// Strategy for loading permissions for a `(user_id, organisation_id)` pair.
#[async_trait]
pub trait PermissionLoaderStrategy: Send + Sync + 'static {
    async fn load(&self, user_id: Uuid, organisation_id: Uuid) -> anyhow::Result<Vec<String>>;
}

/// Wrapper so the layer can hold either a DB-backed or static implementation.
#[derive(Clone)]
pub struct PermissionLoader(Arc<dyn PermissionLoaderStrategy>);

impl PermissionLoader {
    pub fn new<T: PermissionLoaderStrategy>(inner: T) -> Self {
        Self(Arc::new(inner))
    }

    pub fn pg(pool: PgPool) -> Self {
        Self::new(PgPermissionLoader { pool })
    }

    pub fn static_codes(codes: Vec<String>) -> Self {
        Self::new(StaticPermissionLoader { codes: Arc::new(codes) })
    }

    pub async fn load(&self, user: Uuid, org: Uuid) -> anyhow::Result<Vec<String>> {
        self.0.load(user, org).await
    }
}

pub struct PgPermissionLoader { pool: PgPool }

#[async_trait]
impl PermissionLoaderStrategy for PgPermissionLoader {
    async fn load(&self, user_id: Uuid, organisation_id: Uuid) -> anyhow::Result<Vec<String>> {
        let codes: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT p.code
            FROM user_organisation_roles uor
            JOIN role_permissions rp ON rp.role_id = uor.role_id
            JOIN permissions p       ON p.id       = rp.permission_id
            WHERE uor.user_id = $1 AND uor.organisation_id = $2
            "#,
        )
        .bind(user_id)
        .bind(organisation_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(codes)
    }
}

pub struct StaticPermissionLoader { codes: Arc<Vec<String>> }

#[async_trait]
impl PermissionLoaderStrategy for StaticPermissionLoader {
    async fn load(&self, _user: Uuid, _org: Uuid) -> anyhow::Result<Vec<String>> {
        Ok(self.codes.as_ref().clone())
    }
}

#[derive(Clone)]
pub struct AuthLayer {
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
}

impl AuthLayer {
    pub fn new(secret: String, issuer: String, loader: PermissionLoader) -> Self {
        Self { secret: Arc::new(secret), issuer: Arc::new(issuer), loader }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        AuthService { inner, secret: self.secret.clone(), issuer: self.issuer.clone(), loader: self.loader.clone() }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + Into<axum::BoxError> + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let secret = self.secret.clone();
        let issuer = self.issuer.clone();
        let loader = self.loader.clone();

        Box::pin(async move {
            // Extract bearer token
            let token = match req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
                Some(h) if h.starts_with("Bearer ") => h["Bearer ".len()..].to_string(),
                _ => {
                    return Ok(AppError::Unauthenticated { reason: "missing_bearer".into() }
                        .into_response());
                }
            };

            // Decode
            let claims = match decode_access_token(&secret, &issuer, &token) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(AppError::Unauthenticated { reason: "invalid_token".into() }
                        .into_response());
                }
            };

            // Load permissions
            let codes = match loader.load(claims.sub, claims.org).await {
                Ok(c) => c,
                Err(err) => {
                    tracing::error!(error = %err, "permission loader failed");
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
                }
            };
            let perms = PermissionSet::from_codes(codes);

            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(perms);

            inner.call(req).await
        })
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test auth_middleware_test`
Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/auth/middleware.rs tests/auth_middleware_test.rs
git commit -m "feat(auth): AuthLayer with pluggable permission loader"
```

---

### Task 15: `audit/model.rs` — types

**Files:**
- Create: `src/audit/mod.rs`
- Create: `src/audit/model.rs`
- Create: `tests/audit_model_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/audit_model_test.rs`:

```rust
use egras::audit::model::{AuditCategory, AuditEvent, Outcome};

#[test]
fn category_as_str_matches_db_values() {
    assert_eq!(AuditCategory::SecurityStateChange.as_str(), "security.state_change");
    assert_eq!(AuditCategory::SecurityAuth.as_str(),         "security.auth");
    assert_eq!(AuditCategory::SecurityPermissionDenial.as_str(), "security.permission_denial");
    assert_eq!(AuditCategory::TenantsStateChange.as_str(),  "tenants.state_change");
}

#[test]
fn outcome_as_str() {
    assert_eq!(Outcome::Success.as_str(), "success");
    assert_eq!(Outcome::Failure.as_str(), "failure");
    assert_eq!(Outcome::Denied.as_str(),  "denied");
}

#[test]
fn user_registered_event_shape() {
    let e = AuditEvent::user_registered_success(
        uuid::Uuid::now_v7(),  // actor user
        uuid::Uuid::now_v7(),  // actor org
        uuid::Uuid::now_v7(),  // target user
        uuid::Uuid::now_v7(),  // target org
        "org_member".into(),
    );
    assert_eq!(e.category, AuditCategory::SecurityStateChange);
    assert_eq!(e.event_type, "user.registered");
    assert_eq!(e.outcome, Outcome::Success);
    assert_eq!(e.target_type.as_deref(), Some("user"));
    assert_eq!(e.payload["role_code"], "org_member");
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --test audit_model_test`
Expected: compile error.

- [ ] **Step 3: Create `src/audit/mod.rs`**

```rust
pub mod model;
pub mod persistence;
pub mod service;
pub mod worker;
```

- [ ] **Step 4: Create `src/audit/model.rs`**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditCategory {
    SecurityStateChange,
    SecurityAuth,
    SecurityPermissionDenial,
    TenantsStateChange,
}

impl AuditCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SecurityStateChange       => "security.state_change",
            Self::SecurityAuth              => "security.auth",
            Self::SecurityPermissionDenial  => "security.permission_denial",
            Self::TenantsStateChange        => "tenants.state_change",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "security.state_change"      => Self::SecurityStateChange,
            "security.auth"              => Self::SecurityAuth,
            "security.permission_denial" => Self::SecurityPermissionDenial,
            "tenants.state_change"       => Self::TenantsStateChange,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Success,
    Failure,
    Denied,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Denied  => "denied",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "success" => Self::Success,
            "failure" => Self::Failure,
            "denied"  => Self::Denied,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub user_id: Option<Uuid>,
    pub organisation_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl Actor {
    pub fn system() -> Self {
        Self { user_id: None, organisation_id: None, request_id: None, ip_address: None, user_agent: None }
    }
}

/// An audit event ready to be recorded. Use `AuditEvent::*` constructors to build these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub category: AuditCategory,
    pub event_type: String,
    pub actor_user_id: Option<Uuid>,
    pub actor_organisation_id: Option<Uuid>,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub target_organisation_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub outcome: Outcome,
    pub reason_code: Option<String>,
    pub payload: Value,
}

impl AuditEvent {
    fn base(
        category: AuditCategory,
        event_type: &str,
        outcome: Outcome,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            category,
            event_type: event_type.to_string(),
            actor_user_id: None,
            actor_organisation_id: None,
            target_type: None,
            target_id: None,
            target_organisation_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            outcome,
            reason_code: None,
            payload: json!({}),
        }
    }

    pub fn with_actor(mut self, actor: &Actor) -> Self {
        self.actor_user_id = actor.user_id;
        self.actor_organisation_id = actor.organisation_id;
        self.request_id = actor.request_id.clone();
        self.ip_address = actor.ip_address.clone();
        self.user_agent = actor.user_agent.clone();
        self
    }

    pub fn user_registered_success(
        actor_user: Uuid,
        actor_org: Uuid,
        target_user: Uuid,
        target_org: Uuid,
        role_code: String,
    ) -> Self {
        let mut e = Self::base(AuditCategory::SecurityStateChange, "user.registered", Outcome::Success);
        e.actor_user_id = Some(actor_user);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(target_user);
        e.target_organisation_id = Some(target_org);
        e.payload = json!({ "role_code": role_code });
        e
    }

    pub fn login_success(user_id: Uuid, active_org: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::SecurityAuth, "login.success", Outcome::Success);
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(active_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e
    }

    pub fn login_failed(reason_code: &str, username_or_email: &str) -> Self {
        let mut e = Self::base(AuditCategory::SecurityAuth, "login.failed", Outcome::Failure);
        e.reason_code = Some(reason_code.into());
        e.payload = json!({ "username_or_email": username_or_email });
        e
    }

    pub fn logout(user_id: Uuid, org: Uuid, jti: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::SecurityAuth, "logout", Outcome::Success);
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(org);
        e.payload = json!({ "jti": jti });
        e
    }

    pub fn session_switched_org(user_id: Uuid, from_org: Uuid, to_org: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::SecurityAuth, "session.switched_org", Outcome::Success);
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(to_org);
        e.target_type = Some("organisation".into());
        e.target_id = Some(to_org);
        e.target_organisation_id = Some(to_org);
        e.payload = json!({ "from_org": from_org });
        e
    }

    pub fn password_changed(user_id: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::SecurityStateChange, "password.changed", Outcome::Success);
        e.actor_user_id = Some(user_id);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e
    }

    pub fn password_reset_requested(email: &str) -> Self {
        let mut e = Self::base(AuditCategory::SecurityStateChange, "password.reset_requested", Outcome::Success);
        e.payload = json!({ "email": email });
        e
    }

    pub fn password_reset_confirmed(user_id: Option<Uuid>, outcome: Outcome, reason: Option<String>) -> Self {
        let mut e = Self::base(AuditCategory::SecurityStateChange, "password.reset_confirmed", outcome);
        e.actor_user_id = user_id;
        e.reason_code = reason;
        e
    }

    pub fn permission_denied(user_id: Uuid, org: Uuid, permission: &str, path: &str) -> Self {
        let mut e = Self::base(AuditCategory::SecurityPermissionDenial, "permission.denied", Outcome::Denied);
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(org);
        e.reason_code = Some(format!("missing:{permission}"));
        e.payload = json!({ "path": path });
        e
    }

    pub fn organisation_created(actor: Uuid, actor_org: Uuid, org_id: Uuid, name: &str) -> Self {
        let mut e = Self::base(AuditCategory::TenantsStateChange, "organisation.created", Outcome::Success);
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("organisation".into());
        e.target_id = Some(org_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "name": name });
        e
    }

    pub fn organisation_member_added(actor: Uuid, actor_org: Uuid, org_id: Uuid, user_id: Uuid, role_code: &str) -> Self {
        let mut e = Self::base(AuditCategory::TenantsStateChange, "organisation.member_added", Outcome::Success);
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "role_code": role_code });
        e
    }

    pub fn organisation_member_removed(actor: Uuid, actor_org: Uuid, org_id: Uuid, user_id: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::TenantsStateChange, "organisation.member_removed", Outcome::Success);
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e
    }

    pub fn organisation_role_assigned(actor: Uuid, actor_org: Uuid, org_id: Uuid, user_id: Uuid, role_code: &str) -> Self {
        let mut e = Self::base(AuditCategory::TenantsStateChange, "organisation.role_assigned", Outcome::Success);
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "role_code": role_code });
        e
    }
}
```

- [ ] **Step 5: Update `src/lib.rs`**

```rust
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test audit_model_test`
Expected: all 3 tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/audit/mod.rs src/audit/model.rs tests/audit_model_test.rs src/lib.rs
git commit -m "feat(audit): event model, categories, outcomes, and constructors"
```

Note: the submodules `persistence`, `service`, `worker` are declared in `src/audit/mod.rs` but do not yet exist — the next tasks create them before running `cargo check`. If you need compilation to succeed **now**, temporarily comment out the three `pub mod …` lines and uncomment them as each task lands.

---

### Task 16: `audit/persistence` — trait + pg impl

**Files:**
- Create: `src/audit/persistence/mod.rs`
- Create: `src/audit/persistence/audit_repository_pg.rs`
- Create: `tests/audit_persistence_test.rs`

Cursor pagination uses `(occurred_at, id)` tuples per spec §8.5, encoded as base64url JSON.

- [ ] **Step 1: Write `src/audit/persistence/mod.rs`**

```rust
mod audit_repository_pg;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::model::AuditEvent;

pub use audit_repository_pg::AuditRepositoryPg;

/// Query filter for `list_events`. All fields optional except pagination.
#[derive(Debug, Clone, Default)]
pub struct AuditQueryFilter {
    pub organisation_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub category: Option<String>,
    pub outcome: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<AuditCursor>,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditCursor {
    pub occurred_at: DateTime<Utc>,
    pub id: Uuid,
}

impl AuditCursor {
    pub fn encode(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let json = serde_json::to_vec(self).expect("cursor serialises");
        URL_SAFE_NO_PAD.encode(json)
    }

    pub fn decode(s: &str) -> anyhow::Result<Self> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let bytes = URL_SAFE_NO_PAD.decode(s)?;
        let c: AuditCursor = serde_json::from_slice(&bytes)?;
        Ok(c)
    }
}

pub struct AuditQueryPage {
    pub items: Vec<AuditEvent>,
    pub next_cursor: Option<AuditCursor>,
}

#[async_trait]
pub trait AuditRepository: Send + Sync + 'static {
    async fn insert(&self, event: &AuditEvent) -> anyhow::Result<()>;
    async fn list_events(&self, filter: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage>;
}
```

- [ ] **Step 2: Write `src/audit/persistence/audit_repository_pg.rs`**

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::types::ipnetwork::IpNetwork;
use std::str::FromStr;
use uuid::Uuid;

use crate::audit::model::{AuditCategory, AuditEvent, Outcome};
use super::{AuditCursor, AuditQueryFilter, AuditQueryPage, AuditRepository};

pub struct AuditRepositoryPg {
    pool: PgPool,
}

impl AuditRepositoryPg {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[async_trait]
impl AuditRepository for AuditRepositoryPg {
    async fn insert(&self, e: &AuditEvent) -> anyhow::Result<()> {
        let ip: Option<IpNetwork> = e.ip_address.as_deref()
            .and_then(|s| IpNetwork::from_str(s).ok());

        sqlx::query(
            r#"
            INSERT INTO audit_events
              (id, occurred_at, category, event_type,
               actor_user_id, actor_organisation_id,
               target_type, target_id, target_organisation_id,
               request_id, ip_address, user_agent,
               outcome, reason_code, payload)
            VALUES
              ($1, $2, $3, $4,
               $5, $6,
               $7, $8, $9,
               $10, $11, $12,
               $13, $14, $15)
            "#,
        )
        .bind(e.id)
        .bind(e.occurred_at)
        .bind(e.category.as_str())
        .bind(&e.event_type)
        .bind(e.actor_user_id)
        .bind(e.actor_organisation_id)
        .bind(&e.target_type)
        .bind(e.target_id)
        .bind(e.target_organisation_id)
        .bind(&e.request_id)
        .bind(ip)
        .bind(&e.user_agent)
        .bind(e.outcome.as_str())
        .bind(&e.reason_code)
        .bind(&e.payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_events(&self, f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        // Build query with dynamic filters. Parameters are bound positionally; we
        // use a QueryBuilder to keep things readable and injection-safe.
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, occurred_at, category, event_type, \
                    actor_user_id, actor_organisation_id, \
                    target_type, target_id, target_organisation_id, \
                    request_id, host(ip_address) AS ip_address, user_agent, \
                    outcome, reason_code, payload \
             FROM audit_events WHERE 1=1 ",
        );

        if let Some(org) = f.organisation_id {
            qb.push(" AND target_organisation_id = "); qb.push_bind(org);
        }
        if let Some(actor) = f.actor_user_id {
            qb.push(" AND actor_user_id = "); qb.push_bind(actor);
        }
        if let Some(ref et) = f.event_type {
            qb.push(" AND event_type = "); qb.push_bind(et);
        }
        if let Some(ref cat) = f.category {
            qb.push(" AND category = "); qb.push_bind(cat);
        }
        if let Some(ref out) = f.outcome {
            qb.push(" AND outcome = "); qb.push_bind(out);
        }
        if let Some(from) = f.from {
            qb.push(" AND occurred_at >= "); qb.push_bind(from);
        }
        if let Some(to) = f.to {
            qb.push(" AND occurred_at <= "); qb.push_bind(to);
        }
        if let Some(ref c) = f.cursor {
            qb.push(" AND (occurred_at, id) < (");
            qb.push_bind(c.occurred_at).push(", ").push_bind(c.id).push(")");
        }

        qb.push(" ORDER BY occurred_at DESC, id DESC LIMIT ");
        qb.push_bind(f.limit + 1); // fetch one extra to determine next_cursor

        let rows = qb.build().fetch_all(&self.pool).await?;

        let mut items: Vec<AuditEvent> = Vec::with_capacity(rows.len().min(f.limit as usize));
        for row in rows.iter().take(f.limit as usize) {
            use sqlx::Row;
            let category_str: String = row.try_get("category")?;
            let outcome_str: String   = row.try_get("outcome")?;
            items.push(AuditEvent {
                id: row.try_get("id")?,
                occurred_at: row.try_get("occurred_at")?,
                category: AuditCategory::try_from_str(&category_str)
                    .ok_or_else(|| anyhow::anyhow!("unknown category: {category_str}"))?,
                event_type: row.try_get("event_type")?,
                actor_user_id: row.try_get("actor_user_id")?,
                actor_organisation_id: row.try_get("actor_organisation_id")?,
                target_type: row.try_get("target_type")?,
                target_id: row.try_get("target_id")?,
                target_organisation_id: row.try_get("target_organisation_id")?,
                request_id: row.try_get("request_id")?,
                ip_address: row.try_get::<Option<String>, _>("ip_address")?,
                user_agent: row.try_get("user_agent")?,
                outcome: Outcome::try_from_str(&outcome_str)
                    .ok_or_else(|| anyhow::anyhow!("unknown outcome: {outcome_str}"))?,
                reason_code: row.try_get("reason_code")?,
                payload: row.try_get("payload")?,
            });
        }

        let next_cursor = if rows.len() as i64 > f.limit {
            items.last().map(|last| AuditCursor { occurred_at: last.occurred_at, id: last.id })
        } else {
            None
        };

        Ok(AuditQueryPage { items, next_cursor })
    }
}
```

Note: sqlx with `ipnetwork` feature is needed for `INET` columns. Update `Cargo.toml` to include it if not already:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "json", "macros", "migrate", "ipnetwork"] }
ipnetwork = "0.20"
```

- [ ] **Step 3: Update `Cargo.toml` with the ipnetwork feature and dep**

Edit `Cargo.toml` to add the `ipnetwork` feature to sqlx and add `ipnetwork = "0.20"` under `[dependencies]`.

- [ ] **Step 4: Write `tests/audit_persistence_test.rs`**

This test requires a real Postgres (testcontainers). It will be skipped gracefully if Docker is unavailable. It depends on `TestPool` which is built in Task 21 — schedule this test's creation to run AFTER Task 21 (or stub it now and fill in once `TestPool` exists).

Write the test file now, skeleton only:

```rust
use egras::audit::model::{AuditCategory, AuditEvent, Outcome};
use egras::audit::persistence::{AuditQueryFilter, AuditRepository, AuditRepositoryPg};
use egras::testing::TestPool;
use uuid::Uuid;

#[tokio::test]
async fn insert_then_list_roundtrip() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    let org = Uuid::now_v7();
    let actor = Uuid::now_v7();
    let e = AuditEvent::organisation_created(actor, org, org, "acme");

    repo.insert(&e).await.unwrap();

    let page = repo
        .list_events(&AuditQueryFilter { limit: 10, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event_type, "organisation.created");
    assert_eq!(page.items[0].outcome, Outcome::Success);
    assert_eq!(page.items[0].category, AuditCategory::TenantsStateChange);
}

#[tokio::test]
async fn filter_by_event_type() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    let actor = Uuid::now_v7();
    let org = Uuid::now_v7();
    repo.insert(&AuditEvent::login_success(actor, org)).await.unwrap();
    repo.insert(&AuditEvent::login_failed("invalid_credentials", "bob")).await.unwrap();

    let page = repo.list_events(&AuditQueryFilter {
        event_type: Some("login.failed".into()),
        limit: 10,
        ..Default::default()
    }).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event_type, "login.failed");
}

#[tokio::test]
async fn cursor_pagination_terminates() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    for i in 0..5 {
        let mut e = AuditEvent::login_failed("invalid_credentials", &format!("user{i}"));
        // Force distinct occurred_at so ordering is deterministic
        e.occurred_at = chrono::Utc::now() - chrono::Duration::seconds(i);
        repo.insert(&e).await.unwrap();
    }

    let first = repo.list_events(&AuditQueryFilter { limit: 2, ..Default::default() }).await.unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(first.next_cursor.is_some());

    let second = repo.list_events(&AuditQueryFilter {
        limit: 2,
        cursor: first.next_cursor.clone(),
        ..Default::default()
    }).await.unwrap();
    assert_eq!(second.items.len(), 2);

    let third = repo.list_events(&AuditQueryFilter {
        limit: 2,
        cursor: second.next_cursor.clone(),
        ..Default::default()
    }).await.unwrap();
    assert_eq!(third.items.len(), 1);
    assert!(third.next_cursor.is_none());
}
```

- [ ] **Step 5: Confirm failure / defer**

`TestPool` lives in `src/testing.rs` which doesn't exist yet (Task 21). Mark this test file with `#[cfg(feature = "testing")]` gating or simply run:

```bash
cargo test --features testing --test audit_persistence_test
```

Expected at this point: compile error on `egras::testing`. That's OK — we'll wire this up in Task 21 and re-run. Commit the test file as pending.

- [ ] **Step 6: Commit**

```bash
git add src/audit/persistence tests/audit_persistence_test.rs Cargo.toml
git commit -m "feat(audit): AuditRepository trait and Postgres impl with cursor pagination"
```

---

### Task 17: `audit/service/record_event.rs` — `AuditRecorder` + `ChannelAuditRecorder`

**Files:**
- Create: `src/audit/service/mod.rs`
- Create: `src/audit/service/record_event.rs`
- Create: `tests/audit_recorder_test.rs`

- [ ] **Step 1: Write `src/audit/service/mod.rs`**

```rust
pub mod record_event;
pub mod list_audit_events;

pub use record_event::{AuditRecorder, ChannelAuditRecorder, RecorderError};
pub use list_audit_events::{ListAuditEvents, ListAuditEventsImpl, ListAuditEventsRequest, ListAuditEventsResponse};
```

- [ ] **Step 2: Write `src/audit/service/record_event.rs`**

```rust
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::Sender;

use crate::audit::model::AuditEvent;

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("audit channel full; event dropped")]
    ChannelFull,
    #[error("audit channel closed; recorder no longer accepts events")]
    Closed,
}

/// Service trait: services inject `Arc<dyn AuditRecorder>` and call `record` at outcome points.
#[async_trait]
pub trait AuditRecorder: Send + Sync + 'static {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError>;
}

/// Production recorder: non-blocking enqueue onto a bounded mpsc.
pub struct ChannelAuditRecorder {
    tx: Sender<AuditEvent>,
}

impl ChannelAuditRecorder {
    pub fn new(tx: Sender<AuditEvent>) -> Self { Self { tx } }
}

#[async_trait]
impl AuditRecorder for ChannelAuditRecorder {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError> {
        // Mirror to structured log regardless of channel state (spec §7.1, §13).
        tracing::info!(
            target: "egras::audit",
            event_id   = %event.id,
            occurred_at = %event.occurred_at,
            category   = event.category.as_str(),
            event_type = %event.event_type,
            outcome    = event.outcome.as_str(),
            reason_code = ?event.reason_code,
            actor_user_id = ?event.actor_user_id,
            actor_org_id  = ?event.actor_organisation_id,
            target_type   = ?event.target_type,
            target_id     = ?event.target_id,
            target_org_id = ?event.target_organisation_id,
            payload       = %event.payload,
            "audit"
        );
        match self.tx.try_send(event) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(e)) => {
                tracing::error!(
                    event_id = %e.id, event_type = %e.event_type,
                    "audit channel full; dropping event"
                );
                Err(RecorderError::ChannelFull)
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(RecorderError::Closed),
        }
    }
}
```

- [ ] **Step 3: Write `tests/audit_recorder_test.rs`**

```rust
use egras::audit::model::AuditEvent;
use egras::audit::service::{AuditRecorder, ChannelAuditRecorder, RecorderError};
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test]
async fn records_into_channel() {
    let (tx, mut rx) = mpsc::channel(4);
    let rec = ChannelAuditRecorder::new(tx);

    let e = AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7());
    rec.record(e.clone()).await.unwrap();

    let got = rx.recv().await.unwrap();
    assert_eq!(got.id, e.id);
    assert_eq!(got.event_type, "login.success");
}

#[tokio::test]
async fn returns_channel_full_when_buffer_exhausted() {
    let (tx, _rx) = mpsc::channel(1);
    let rec = ChannelAuditRecorder::new(tx);

    // First send fills the buffer.
    rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    // Second send returns ChannelFull.
    let err = rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap_err();
    assert!(matches!(err, RecorderError::ChannelFull));
}

#[tokio::test]
async fn returns_closed_after_receiver_drop() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let rec = ChannelAuditRecorder::new(tx);
    let err = rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7()))
        .await.unwrap_err();
    assert!(matches!(err, RecorderError::Closed));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test audit_recorder_test`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/audit/service/mod.rs src/audit/service/record_event.rs tests/audit_recorder_test.rs
git commit -m "feat(audit): AuditRecorder trait and ChannelAuditRecorder"
```

Note: `list_audit_events` module is declared in `service/mod.rs` but not yet written — Task 19 adds it. Either temporarily comment out that `pub mod` line here, or write a minimal stub (empty file) and flesh it out in Task 19. Prefer the stub approach — less churn.

---

### Task 18: `audit/worker.rs` — `AuditWorker` with retry + handle

**Files:**
- Create: `src/audit/worker.rs`
- Create: `tests/audit_worker_test.rs`

- [ ] **Step 1: Write `src/audit/worker.rs`**

```rust
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

use crate::audit::model::AuditEvent;
use crate::audit::persistence::AuditRepository;

pub struct AuditWorker {
    rx: Receiver<AuditEvent>,
    repo: Arc<dyn AuditRepository>,
    max_retries: u32,
    backoff_initial_ms: u64,
}

pub struct AuditWorkerHandle {
    task: JoinHandle<()>,
}

impl AuditWorkerHandle {
    /// Wait for the worker task to complete draining after the sender is dropped/closed.
    pub async fn shutdown(self) {
        if let Err(err) = self.task.await {
            tracing::error!(error = %err, "audit worker task join error");
        }
    }
}

impl AuditWorker {
    pub fn new(
        rx: Receiver<AuditEvent>,
        repo: Arc<dyn AuditRepository>,
        max_retries: u32,
        backoff_initial_ms: u64,
    ) -> Self {
        Self { rx, repo, max_retries, backoff_initial_ms }
    }

    pub fn spawn(self) -> AuditWorkerHandle {
        let task = tokio::spawn(self.run());
        AuditWorkerHandle { task }
    }

    async fn run(mut self) {
        tracing::info!("audit worker started");
        while let Some(event) = self.rx.recv().await {
            self.write_with_retry(event).await;
        }
        tracing::info!("audit worker stopped (channel closed, queue drained)");
    }

    async fn write_with_retry(&self, event: AuditEvent) {
        let mut attempt: u32 = 0;
        let mut backoff_ms = self.backoff_initial_ms;
        loop {
            match self.repo.insert(&event).await {
                Ok(()) => return,
                Err(err) => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        tracing::error!(
                            event_id = %event.id,
                            event_type = %event.event_type,
                            attempt,
                            error = %err,
                            payload = %event.payload,
                            "audit worker: permanent failure, dropping event"
                        );
                        return;
                    }
                    tracing::warn!(
                        event_id = %event.id,
                        attempt,
                        error = %err,
                        "audit worker: retryable failure"
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = backoff_ms.saturating_mul(4);
                }
            }
        }
    }
}
```

- [ ] **Step 2: Write `tests/audit_worker_test.rs`**

Unit-testable without a DB: use a mock `AuditRepository` via `mockall`.

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use egras::audit::model::AuditEvent;
use egras::audit::persistence::{AuditRepository, AuditQueryFilter, AuditQueryPage};
use egras::audit::worker::AuditWorker;
use tokio::sync::mpsc;
use uuid::Uuid;

struct AlwaysOkRepo { calls: Arc<AtomicU32> }

#[async_trait]
impl AuditRepository for AlwaysOkRepo {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn list_events(&self, _f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        unimplemented!()
    }
}

struct FailsNTimes {
    remaining_failures: Arc<AtomicU32>,
    successes: Arc<AtomicU32>,
}

#[async_trait]
impl AuditRepository for FailsNTimes {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> {
        if self.remaining_failures.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            if n == 0 { None } else { Some(n - 1) }
        }).is_ok() {
            anyhow::bail!("transient failure");
        }
        self.successes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn list_events(&self, _f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        unimplemented!()
    }
}

#[tokio::test]
async fn drains_all_events_on_shutdown() {
    let (tx, rx) = mpsc::channel(16);
    let calls = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(AlwaysOkRepo { calls: calls.clone() });
    let handle = AuditWorker::new(rx, repo, 3, 5).spawn();

    for _ in 0..5 {
        tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    }
    drop(tx);
    handle.shutdown().await;

    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn retries_then_succeeds() {
    let (tx, rx) = mpsc::channel(4);
    let successes = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(FailsNTimes {
        remaining_failures: Arc::new(AtomicU32::new(2)),
        successes: successes.clone(),
    });
    let handle = AuditWorker::new(rx, repo, 5, 1).spawn();

    tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    drop(tx);
    handle.shutdown().await;

    assert_eq!(successes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let (tx, rx) = mpsc::channel(4);
    let successes = Arc::new(AtomicU32::new(0));
    let repo = Arc::new(FailsNTimes {
        remaining_failures: Arc::new(AtomicU32::new(100)), // always fail
        successes: successes.clone(),
    });
    let handle = AuditWorker::new(rx, repo, 2, 1).spawn();

    tx.send(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    drop(tx);
    handle.shutdown().await;

    assert_eq!(successes.load(Ordering::SeqCst), 0); // never succeeded
    // Worker still terminated (did not block forever)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test audit_worker_test`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/audit/worker.rs tests/audit_worker_test.rs
git commit -m "feat(audit): AuditWorker with retry, backoff, and graceful drain"
```

---

### Task 19: `audit/service/list_audit_events.rs` — query use case

**Files:**
- Create: `src/audit/service/list_audit_events.rs`
- Create: `tests/audit_list_events_test.rs`

The HTTP handler lives in Plan 3. This task defines the service trait + impl consumed by the (future) handler.

- [ ] **Step 1: Write `src/audit/service/list_audit_events.rs`**

```rust
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditCursor, AuditQueryFilter, AuditRepository};
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct ListAuditEventsRequest {
    pub organisation_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub category: Option<String>,
    pub outcome: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListAuditEventsResponse {
    pub items: Vec<AuditEvent>,
    pub next_cursor: Option<String>,
}

#[async_trait]
pub trait ListAuditEvents: Send + Sync + 'static {
    /// `caller_org` is the `org` from the caller's JWT; `perms` is their PermissionSet.
    /// Caller must hold `audit.read_own_org` or `audit.read_all`; this is enforced here.
    async fn execute(
        &self,
        req: ListAuditEventsRequest,
        caller_org: Uuid,
        perms: &PermissionSet,
    ) -> Result<ListAuditEventsResponse, AppError>;
}

pub struct ListAuditEventsImpl {
    repo: Arc<dyn AuditRepository>,
}

impl ListAuditEventsImpl {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self { Self { repo } }
}

#[async_trait]
impl ListAuditEvents for ListAuditEventsImpl {
    async fn execute(
        &self,
        req: ListAuditEventsRequest,
        caller_org: Uuid,
        perms: &PermissionSet,
    ) -> Result<ListAuditEventsResponse, AppError> {
        // Authorisation (spec §7.5):
        //   audit.read_all → any organisation_id (None ⇒ all orgs)
        //   audit.read_own_org → organisation_id must equal caller_org or be None (→ resolve to caller_org)
        let effective_org_filter: Option<Uuid> = if perms.is_audit_read_all() {
            req.organisation_id
        } else if perms.has("audit.read_own_org") {
            match req.organisation_id {
                None => Some(caller_org),
                Some(o) if o == caller_org => Some(o),
                Some(_) => return Err(AppError::NotFound { resource: "organisation".into() }),
            }
        } else {
            return Err(AppError::PermissionDenied { code: "audit.read_own_org".into() });
        };

        let limit = req.limit.unwrap_or(100).clamp(1, 200);
        let cursor = match req.cursor.as_deref() {
            Some(s) => Some(AuditCursor::decode(s).map_err(|_| AppError::Validation {
                errors: [("cursor".to_string(), vec!["invalid".to_string()])].into_iter().collect(),
            })?),
            None => None,
        };

        let filter = AuditQueryFilter {
            organisation_id: effective_org_filter,
            actor_user_id: req.actor_user_id,
            event_type: req.event_type,
            category: req.category,
            outcome: req.outcome,
            from: req.from,
            to: req.to,
            cursor,
            limit,
        };

        let page = self.repo.list_events(&filter).await.map_err(AppError::Internal)?;
        Ok(ListAuditEventsResponse {
            items: page.items,
            next_cursor: page.next_cursor.map(|c| c.encode()),
        })
    }
}
```

- [ ] **Step 2: Write `tests/audit_list_events_test.rs`**

Uses `mockall`-generated mock `AuditRepository`.

```rust
use std::sync::Arc;

use async_trait::async_trait;
use egras::audit::model::AuditEvent;
use egras::audit::persistence::{AuditQueryFilter, AuditQueryPage, AuditRepository};
use egras::audit::service::{ListAuditEvents, ListAuditEventsImpl, ListAuditEventsRequest};
use egras::auth::permissions::PermissionSet;
use uuid::Uuid;

struct StubRepo {
    captured: std::sync::Mutex<Option<AuditQueryFilter>>,
}

#[async_trait]
impl AuditRepository for StubRepo {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> { Ok(()) }
    async fn list_events(&self, f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        *self.captured.lock().unwrap() = Some(f.clone());
        Ok(AuditQueryPage { items: vec![], next_cursor: None })
    }
}

#[tokio::test]
async fn read_all_can_query_any_org() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_all".into()]);
    let target_org = Uuid::now_v7();

    svc.execute(
        ListAuditEventsRequest {
            organisation_id: Some(target_org),
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        Uuid::now_v7(),
        &perms,
    ).await.unwrap();

    let f = repo.captured.lock().unwrap().clone().unwrap();
    assert_eq!(f.organisation_id, Some(target_org));
}

#[tokio::test]
async fn read_own_org_with_null_resolves_to_caller_org() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_own_org".into()]);
    let caller_org = Uuid::now_v7();

    svc.execute(
        ListAuditEventsRequest {
            organisation_id: None,
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        caller_org,
        &perms,
    ).await.unwrap();

    let f = repo.captured.lock().unwrap().clone().unwrap();
    assert_eq!(f.organisation_id, Some(caller_org));
}

#[tokio::test]
async fn read_own_org_with_foreign_org_returns_not_found() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_own_org".into()]);
    let caller_org = Uuid::now_v7();
    let foreign = Uuid::now_v7();

    let err = svc.execute(
        ListAuditEventsRequest {
            organisation_id: Some(foreign),
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        caller_org,
        &perms,
    ).await.unwrap_err();

    assert!(matches!(err, egras::errors::AppError::NotFound { .. }));
}

#[tokio::test]
async fn no_audit_permission_is_denied() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["tenants.read".into()]);

    let err = svc.execute(
        ListAuditEventsRequest {
            organisation_id: None,
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        Uuid::now_v7(),
        &perms,
    ).await.unwrap_err();

    assert!(matches!(err, egras::errors::AppError::PermissionDenied { .. }));
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test audit_list_events_test`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/audit/service/list_audit_events.rs tests/audit_list_events_test.rs
git commit -m "feat(audit): ListAuditEvents service with permission-scoped filtering"
```

---

### Task 20: `app_state.rs` + `lib.rs::build_app` — wire health/ready + audit worker

**Files:**
- Create: `src/app_state.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs` (was stubbed in Task 11)
- Create: `src/tenants/mod.rs` (empty placeholder)
- Create: `src/security/mod.rs` (empty placeholder)

`AppState` is deliberately minimal in Plan 1 — it holds only the audit infrastructure. Plan 2 will extend it with domain service trait objects.

- [ ] **Step 1: Write `src/app_state.rs`**

```rust
use std::sync::Arc;

use sqlx::PgPool;

use crate::audit::service::{AuditRecorder, ListAuditEvents};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    // Plan 2 will add: register_user, login, change_password, switch_org,
    //                   create_organisation, add_user_to_organisation, etc.
}
```

- [ ] **Step 2: Create placeholder domain modules**

`src/tenants/mod.rs`:

```rust
//! Tenants domain — populated in Plan 2.
```

`src/security/mod.rs`:

```rust
//! Security domain — populated in Plan 2.
```

- [ ] **Step 3: Write `src/lib.rs`**

Replace previous contents with the full module re-exports and `build_app`:

```rust
pub mod app_state;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
pub mod security;
pub mod tenants;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::app_state::AppState;
use crate::audit::persistence::AuditRepositoryPg;
use crate::audit::service::{ChannelAuditRecorder, ListAuditEventsImpl};
use crate::audit::worker::{AuditWorker, AuditWorkerHandle};
use crate::auth::middleware::{AuthLayer, PermissionLoader};
use crate::config::AppConfig;

pub async fn build_app(
    pool: PgPool,
    cfg: AppConfig,
) -> anyhow::Result<(Router, AuditWorkerHandle)> {
    // 1. Audit infra
    let (audit_tx, audit_rx) = mpsc::channel(cfg.audit_channel_capacity);
    let audit_repo: Arc<dyn crate::audit::persistence::AuditRepository> =
        Arc::new(AuditRepositoryPg::new(pool.clone()));
    let audit_handle = AuditWorker::new(
        audit_rx,
        audit_repo.clone(),
        cfg.audit_max_retries,
        cfg.audit_retry_backoff_ms_initial,
    ).spawn();

    let audit_recorder: Arc<dyn crate::audit::service::AuditRecorder> =
        Arc::new(ChannelAuditRecorder::new(audit_tx));
    let list_audit_events: Arc<dyn crate::audit::service::ListAuditEvents> =
        Arc::new(ListAuditEventsImpl::new(audit_repo.clone()));

    let state = AppState {
        pool: pool.clone(),
        audit_recorder,
        list_audit_events,
    };

    // 2. Public routes (no auth)
    let public = Router::new()
        .route("/health", get(health))
        .route("/ready", get({
            let pool = pool.clone();
            move || ready(pool.clone())
        }));

    // 3. Protected routes — empty in Plan 1 (handlers added in Plan 2 & 3)
    let auth_layer = AuthLayer::new(
        cfg.jwt_secret.clone(),
        cfg.jwt_issuer.clone(),
        PermissionLoader::pg(pool.clone()),
    );
    let protected: Router = Router::new().layer(auth_layer);

    // 4. Compose
    let cors = build_cors(&cfg);
    let router = public
        .merge(protected)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    Ok((router, audit_handle))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(pool: PgPool) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool).await {
        Ok(_) => Ok(Json(json!({ "status": "ready" }))),
        Err(err) => {
            tracing::warn!(error = %err, "readiness check failed");
            Err((
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "not_ready", "error": err.to_string() })),
            ))
        }
    }
}

fn build_cors(cfg: &AppConfig) -> CorsLayer {
    if cfg.cors_allowed_origins.trim().is_empty() {
        CorsLayer::new()
    } else {
        let origins: Vec<axum::http::HeaderValue> = cfg.cors_allowed_origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
            .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION])
    }
}
```

- [ ] **Step 4: Verify main.rs now compiles**

Run: `cargo build`
Expected: clean build. The server boots (with only /health and /ready) and the audit worker is spawned.

- [ ] **Step 5: Commit**

```bash
git add src/app_state.rs src/lib.rs src/main.rs src/tenants src/security
git commit -m "feat: build_app wires audit, health/ready, CORS, tracing; AppState skeleton"
```

---

### Task 21: `src/testing.rs` — `TestPool`, `BlockingAuditRecorder`, `mint_jwt`, `MockAppStateBuilder`, `TestApp`

**Files:**
- Create: `src/testing.rs`

This module is feature-gated (`#[cfg(any(test, feature = "testing"))]`) and exposes the helpers used by integration tests. `MockAppStateBuilder` here contains only the audit slots; Plan 2 extends it with domain service slots.

- [ ] **Step 1: Write `src/testing.rs`**

```rust
//! Test helpers. Enabled via the `testing` feature or in test builds.

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditRepository, AuditRepositoryPg};
use crate::audit::service::{AuditRecorder, ListAuditEventsImpl, ListAuditEvents, RecorderError};
use crate::auth::jwt::encode_access_token;
use crate::db::run_migrations;

/// Ephemeral Postgres for tests. Keep the `ContainerAsync` alive for the test's lifetime.
pub struct TestPool {
    pub pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestPool {
    pub async fn fresh() -> Self {
        let container = Postgres::default()
            .with_db_name("egras_test")
            .with_user("egras")
            .with_password("egras")
            .start()
            .await
            .expect("start postgres container");

        let host_port = container.get_host_port_ipv4(5432).await.expect("pg port");
        let url = format!("postgres://egras:egras@127.0.0.1:{host_port}/egras_test");
        let pool = PgPool::connect(&url).await.expect("connect pg");
        run_migrations(&pool).await.expect("migrations");
        Self { pool, _container: container }
    }
}

/// Synchronous audit recorder for E2E tests — writes directly to the DB so the
/// rows are visible to the next query without waiting for the worker.
pub struct BlockingAuditRecorder {
    repo: Arc<dyn AuditRepository>,
    /// Captures events for assertion when DB is not required.
    pub captured: Arc<Mutex<Vec<AuditEvent>>>,
}

impl BlockingAuditRecorder {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self {
        Self { repo, captured: Arc::new(Mutex::new(Vec::new())) }
    }
}

#[async_trait]
impl AuditRecorder for BlockingAuditRecorder {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError> {
        self.captured.lock().await.push(event.clone());
        self.repo.insert(&event).await.map_err(|e| {
            tracing::error!(error = %e, "BlockingAuditRecorder insert failed");
            RecorderError::Closed
        })?;
        Ok(())
    }
}

/// Issue a JWT for tests. Caller owns the permission loading path — see `MockAppStateBuilder`.
pub fn mint_jwt(secret: &str, issuer: &str, user_id: Uuid, org_id: Uuid, ttl_secs: i64) -> String {
    encode_access_token(secret, issuer, user_id, org_id, ttl_secs)
        .expect("mint_jwt failed")
}

/// Builder that produces an `AppState` wired with audit infra for tests. Plan 2
/// extends this with fluent setters for domain service mocks.
pub struct MockAppStateBuilder {
    pool: PgPool,
    audit_recorder: Option<Arc<dyn AuditRecorder>>,
    list_audit_events: Option<Arc<dyn ListAuditEvents>>,
}

impl MockAppStateBuilder {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, audit_recorder: None, list_audit_events: None }
    }

    pub fn with_blocking_audit(mut self) -> Self {
        let repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(self.pool.clone()));
        self.audit_recorder = Some(Arc::new(BlockingAuditRecorder::new(repo.clone())));
        self.list_audit_events = Some(Arc::new(ListAuditEventsImpl::new(repo)));
        self
    }

    pub fn audit_recorder(mut self, rec: Arc<dyn AuditRecorder>) -> Self {
        self.audit_recorder = Some(rec); self
    }

    pub fn list_audit_events(mut self, svc: Arc<dyn ListAuditEvents>) -> Self {
        self.list_audit_events = Some(svc); self
    }

    pub fn build(self) -> AppState {
        AppState {
            pool: self.pool,
            audit_recorder: self.audit_recorder.expect("audit_recorder not set"),
            list_audit_events: self.list_audit_events.expect("list_audit_events not set"),
        }
    }
}

/// A running test server. Holds the join handle and a shutdown sender.
pub struct TestApp {
    pub base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl TestApp {
    /// Spawn `build_app` bound to port 0. Returns base URL "http://127.0.0.1:<port>".
    pub async fn spawn(pool: PgPool, cfg: crate::config::AppConfig) -> Self {
        let (router, audit_handle) = crate::build_app(pool, cfg).await.expect("build_app");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let base_url = format!("http://{addr}");

        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move { rx.await.ok(); });
            let _ = server.await;
            audit_handle.shutdown().await;
        });

        Self { base_url, shutdown: Some(tx), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() { let _ = tx.send(()); }
        if let Some(h) = self.handle.take() { let _ = h.await; }
    }
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo build --features testing`
Expected: clean build.

- [ ] **Step 3: Re-run deferred tests from Task 16**

Run: `cargo test --features testing --test audit_persistence_test`
Expected: 3 tests pass (requires Docker).

- [ ] **Step 4: Commit**

```bash
git add src/testing.rs
git commit -m "feat(testing): TestPool, BlockingAuditRecorder, mint_jwt, MockAppStateBuilder, TestApp"
```

---

### Task 22: `tests/common/*` — shared fixtures & auth helpers

**Files:**
- Create: `tests/common/mod.rs`
- Create: `tests/common/fixtures.rs`
- Create: `tests/common/auth.rs`

Files under `tests/common/` are not auto-compiled; consumers pull them via `mod common;` — but since each `tests/*.rs` file is its own binary, we use the `#[path = ...]` pattern instead for sharing.

- [ ] **Step 1: Write `tests/common/mod.rs`**

```rust
//! Shared helpers for integration tests. Include via:
//!   #[path = "common/mod.rs"]
//!   mod common;
pub mod fixtures;
pub mod auth;
```

- [ ] **Step 2: Write `tests/common/fixtures.rs`**

```rust
use uuid::Uuid;

/// Deterministic UUIDs from migration 0005 — keep in sync.
pub const OPERATOR_ORG_ID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000001);
pub const ROLE_OPERATOR_ADMIN: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000101);
pub const ROLE_ORG_OWNER:      Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000102);
pub const ROLE_ORG_ADMIN:      Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000103);
pub const ROLE_ORG_MEMBER:     Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000104);
```

- [ ] **Step 3: Write `tests/common/auth.rs`**

```rust
use egras::testing::mint_jwt;
use uuid::Uuid;

pub fn bearer(secret: &str, issuer: &str, user: Uuid, org: Uuid) -> String {
    format!("Bearer {}", mint_jwt(secret, issuer, user, org, 3600))
}
```

- [ ] **Step 4: No tests to run (helpers only). Commit.**

```bash
git add tests/common
git commit -m "test: shared fixtures and auth helpers"
```

---

### Task 23: `tests/health_test.rs` — E2E smoke

**Files:**
- Create: `tests/health_test.rs`

- [ ] **Step 1: Write the test**

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};

fn test_config() -> AppConfig {
    AppConfig {
        database_url: String::new(), // unused — we pass the pool directly
        database_max_connections: 5,
        bind_address: "127.0.0.1:0".into(),
        jwt_secret: "a".repeat(64),
        jwt_ttl_secs: 3600,
        jwt_issuer: "egras".into(),
        log_level: "info".into(),
        log_format: "json".into(),
        cors_allowed_origins: String::new(),
        password_reset_ttl_secs: 3600,
        operator_org_name: "operator".into(),
        audit_channel_capacity: 32,
        audit_max_retries: 3,
        audit_retry_backoff_ms_initial: 10,
    }
}

#[tokio::test]
async fn health_returns_ok() {
    let tp = TestPool::fresh().await;
    let app = TestApp::spawn(tp.pool.clone(), test_config()).await;

    let resp = reqwest::get(format!("{}/health", app.base_url)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(v["status"], "ok");

    app.stop().await;
}

#[tokio::test]
async fn ready_returns_ok_when_db_reachable() {
    let tp = TestPool::fresh().await;
    let app = TestApp::spawn(tp.pool.clone(), test_config()).await;

    let resp = reqwest::get(format!("{}/ready", app.base_url)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(v["status"], "ready");

    app.stop().await;
}

#[tokio::test]
async fn migration_0005_seeded_operator_org() {
    let tp = TestPool::fresh().await;
    let row: (String,) = sqlx::query_as(
        "SELECT name FROM organisations WHERE is_operator = TRUE"
    ).fetch_one(&tp.pool).await.unwrap();
    assert_eq!(row.0, "operator");
}

#[tokio::test]
async fn migration_0005_has_all_built_in_roles() {
    let tp = TestPool::fresh().await;
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT code FROM roles WHERE is_builtin = TRUE ORDER BY code"
    ).fetch_all(&tp.pool).await.unwrap();
    let codes: Vec<String> = rows.into_iter().map(|r| r.0).collect();
    assert_eq!(codes, vec!["operator_admin", "org_admin", "org_member", "org_owner"]);
}
```

- [ ] **Step 2: Run**

Run: `cargo test --features testing --test health_test`
Expected: 4 tests pass (requires Docker).

- [ ] **Step 3: Commit**

```bash
git add tests/health_test.rs
git commit -m "test: E2E health/ready and migration sanity"
```

---

### Task 24: Dockerfile + docker-compose.yml

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`

- [ ] **Step 1: Write `Dockerfile`** (from spec §12.1)

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

- [ ] **Step 2: Write `docker-compose.yml`**

```yaml
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: egras
      POSTGRES_PASSWORD: egras
      POSTGRES_DB: egras
    ports:
      - "5432:5432"
    volumes:
      - pg_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U egras -d egras"]
      interval: 5s
      timeout: 3s
      retries: 20

  egras:
    build: .
    depends_on:
      postgres:
        condition: service_healthy
    environment:
      EGRAS_DATABASE_URL: postgres://egras:egras@postgres:5432/egras
      EGRAS_DATABASE_MAX_CONNECTIONS: "10"
      EGRAS_BIND_ADDRESS: 0.0.0.0:8080
      EGRAS_JWT_SECRET: "DEV-ONLY-32-bytes-of-placeholder-entropy"
      EGRAS_JWT_TTL_SECS: "3600"
      EGRAS_JWT_ISSUER: "egras"
      EGRAS_LOG_LEVEL: "info"
      EGRAS_LOG_FORMAT: "json"
      EGRAS_PASSWORD_RESET_TTL_SECS: "3600"
      EGRAS_OPERATOR_ORG_NAME: "operator"
      EGRAS_AUDIT_CHANNEL_CAPACITY: "4096"
      EGRAS_AUDIT_MAX_RETRIES: "3"
      EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL: "100"
    ports:
      - "8080:8080"

volumes:
  pg_data:
```

- [ ] **Step 3: Manual smoke (optional, not in CI)**

Run: `docker compose up --build`
Expected: both services start; `curl http://localhost:8080/health` returns `{"status":"ok"}`. Stop with `docker compose down`.

- [ ] **Step 4: Commit**

```bash
git add Dockerfile docker-compose.yml
git commit -m "chore: multi-stage Dockerfile and docker-compose for local dev"
```

---

### Task 25: `.github/workflows/ci.yml`

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write CI workflow**

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache cargo registry & target
        uses: Swatinem/rust-cache@v2

      - name: fmt
        run: cargo fmt --all -- --check

      - name: clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: test
        run: cargo test --all-features
        env:
          RUST_LOG: info

      - name: build release
        run: cargo build --release
```

OpenAPI drift is added in Plan 3 — `dump-openapi` is a stub today.

- [ ] **Step 2: Verify locally**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
Expected: all three pass.

- [ ] **Step 3: Commit + push to trigger CI**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: fmt / clippy / test / build on push and PR"
```

---

## Acceptance — Plan 1 "done"

- All 25 tasks committed.
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test --all-features` green, including:
  - `errors_test` (8 tests)
  - `config_test` (2)
  - `auth_jwt_test` (4)
  - `auth_permissions_test` (3)
  - `auth_middleware_test` (3)
  - `audit_model_test` (3)
  - `audit_persistence_test` (3)
  - `audit_recorder_test` (3)
  - `audit_worker_test` (3)
  - `audit_list_events_test` (4)
  - `health_test` (4)
- `docker compose up --build` + `curl /health` works.
- GitHub Actions CI green on main.

## Handoff to Plan 2

Plan 2 will:
- Populate `src/tenants/` and `src/security/` (model, persistence, services, handlers).
- Extend `AppState` with domain service trait objects.
- Extend `MockAppStateBuilder` with fluent setters for each service mock.
- Wire the audit recorder into every state-changing use case.

Plan 2 does **not** add permission-denial audit emission (that's Plan 3), list-audit-events HTTP handler (Plan 3), seed-admin CLI (Plan 3), or OpenAPI generation (Plan 3).







