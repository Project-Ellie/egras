---
title: Developer Guide
tags:
  - guide
  - howto
  - development
---

# Developer Guide

Practical steps for common development tasks. Read [[Architecture]] first if you're new to the codebase.

## Running Locally

### Prerequisites

- Rust stable (latest)
- Docker + Docker Compose
- PostgreSQL client (`psql`) — optional, for inspection

### Start the database

```bash
docker-compose up postgres
```

This starts a Postgres 16 container at `localhost:5432` with credentials `egras:egras`.

### Run the server

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/egras \
EGRAS_JWT_SECRET=dev-only-32-bytes-of-placeholder-xx \
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000 \
EGRAS_LOG_FORMAT=pretty \
cargo run
```

Migrations run automatically on startup.

### Seed the first admin user

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/egras \
EGRAS_JWT_SECRET=dev-only-32-bytes-of-placeholder-xx \
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000 \
cargo run -- seed-admin \
  --email admin@example.com \
  --username admin \
  --password "Admin123!"
```

### Open Swagger UI

With the server running: [http://localhost:8080/swagger-ui](http://localhost:8080/swagger-ui)

### Run the test suite

```bash
# Start Postgres if not already running
docker-compose up -d postgres

# Run all tests
TEST_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/postgres \
  cargo test --all-features
```

Note: `TEST_DATABASE_URL` points to the `postgres` database (not `egras`). Each test creates its own isolated database.

### Pre-push checklist

Always run these three before pushing to avoid CI failures:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
TEST_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/postgres \
  cargo test --all-features
```

---

## Add a New Use Case

A "use case" is one service function + its handler. Follow these steps to add, for example, `deactivate_user`.

### Step 1 — Create the service file

`src/security/service/deactivate_user.rs`:

```rust
use uuid::Uuid;
use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct DeactivateUserInput {
    pub target_user_id: Uuid,
    pub org_id:         Uuid,
}

#[derive(Debug, Clone)]
pub struct DeactivateUserOutput {
    pub user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum DeactivateUserError {
    #[error("user not found")]
    UserNotFound,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn deactivate_user(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: DeactivateUserInput,
) -> Result<DeactivateUserOutput, DeactivateUserError> {
    // business logic here
    // ...

    // Emit audit event
    let event = AuditEvent::user_deactivated(actor_user_id, actor_org_id, input.target_user_id);
    let _ = state.audit_recorder.record(event).await;

    Ok(DeactivateUserOutput { user_id: input.target_user_id })
}
```

### Step 2 — Export from the module

`src/security/service/mod.rs`:
```rust
pub mod deactivate_user;
```

### Step 3 — Add a repository method (if needed)

If the use case needs a new DB operation, add it to `UserRepository` trait and implement it in `UserRepositoryPg`.

### Step 4 — Add an audit event constructor (if emitting events)

`src/audit/model.rs`:
```rust
pub fn user_deactivated(actor_user_id: Uuid, actor_org_id: Uuid, target_user_id: Uuid) -> Self {
    let mut e = Self::base(AuditCategory::SecurityStateChange, "user.deactivated", Outcome::Success);
    e.actor_user_id = Some(actor_user_id);
    e.actor_organisation_id = Some(actor_org_id);
    e.target_type = Some("user".into());
    e.target_id = Some(target_user_id);
    e
}
```

### Step 5 — Add the handler

`src/security/interface.rs` — add to the protected router:

```rust
pub fn protected_router() -> Router<AppState> {
    Router::new()
        // existing routes...
        .route("/deactivate-user", post(deactivate_user_handler))
}

#[utoipa::path(post, path = "/api/v1/security/deactivate-user", ...)]
async fn deactivate_user_handler(
    caller: AuthedCaller,
    _perm: Perm<UsersManageAll>,    // whatever permission is appropriate
    State(state): State<AppState>,
    Json(body): Json<DeactivateUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    let output = deactivate_user::deactivate_user(
        &state,
        caller.claims.sub,
        caller.claims.org,
        DeactivateUserInput {
            target_user_id: body.user_id,
            org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        DeactivateUserError::UserNotFound => AppError::NotFound { resource: "user".into() },
        DeactivateUserError::Internal(e) => AppError::Internal(e),
    })?;

    Ok((StatusCode::OK, Json(json!({ "user_id": output.user_id }))))
}
```

### Step 6 — Update OpenAPI

`src/openapi.rs` — add the handler to the `paths(...)` list in the `#[derive(OpenApi)]` macro. Then regenerate the spec:

```bash
EGRAS_DATABASE_URL=... EGRAS_JWT_SECRET=... EGRAS_CORS_ALLOWED_ORIGINS=... \
  cargo run -- dump-openapi > docs/openapi.json
```

### Step 7 — Write tests

At minimum:
- `tests/security_service_deactivate_user_test.rs` — service layer with mock/real state
- `tests/security_http_deactivate_user_test.rs` — HTTP layer with `TestApp`

---

## Add a New Permission

### Step 1 — Add the permission code to the database

Create a new migration file `migrations/0008_add_deactivate_user_permission.sql`:

```sql
INSERT INTO permissions (id, code, created_at)
VALUES (gen_random_uuid(), 'users.deactivate', NOW())
ON CONFLICT DO NOTHING;

-- Assign to operator_admin
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.code = 'operator_admin' AND p.code = 'users.deactivate'
ON CONFLICT DO NOTHING;
```

### Step 2 — Create the permission marker type

`src/auth/extractors.rs`:

```rust
pub struct UsersDeactivate;
impl Permission for UsersDeactivate {
    const CODE: &'static str = "users.deactivate";
    fn accepts(set: &PermissionSet) -> bool {
        // Add operator bypass if appropriate:
        set.has(Self::CODE) || set.is_operator_over_users()
    }
}
```

### Step 3 — Use it in the handler

```rust
_perm: Perm<UsersDeactivate>,
```

---

## Add a New Domain

If you're adding an entirely new domain (e.g., `billing/`):

1. Create `src/billing/` with `mod.rs`, `model.rs`, `interface.rs`, `service/mod.rs`, `persistence/mod.rs`
2. Add `pub mod billing;` to `src/lib.rs`
3. Add the billing repository traits to `AppState` (and `MockAppStateBuilder`)
4. Register the billing router in `build_app()` in `src/lib.rs`
5. Update `src/openapi.rs` to include billing routes

Follow the same file-per-use-case pattern used in `security/` and `tenants/`.

---

## Update the OpenAPI Spec

After any handler change (new endpoint, changed request/response shape), regenerate:

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/egras \
EGRAS_JWT_SECRET=dev-only-32-bytes-of-placeholder-xx \
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000 \
cargo run -- dump-openapi > docs/openapi.json
```

The CI checks that the committed spec matches what `dump-openapi` generates — see [[CI-and-Deployment#OpenAPI drift check]].

---

## Common Pitfalls

> [!warning] Don't forget `cargo fmt`
> `cargo fmt --all -- --check` runs in CI. It's strict about import ordering and line length. Run `cargo fmt --all` before pushing.

> [!warning] Don't use raw SQL in handlers
> Handlers must not hold a `PgPool`. All DB access goes through repository traits on `AppState`. See [[Architecture#Dependency Injection via AppState]].

> [!warning] Audit events are fire-and-forget
> `state.audit_recorder.record(event).await` — the `await` is for the channel send, not the DB write. Don't rely on the event being in the DB immediately after the call (use `BlockingAuditRecorder` in tests — see [[Testing-Strategy#BlockingAuditRecorder]]).

> [!warning] Cross-org access must return 404, not 403
> When a caller accesses a resource in another org they're not authorised for, return 404. See [[Authorization#Org Scoping]] and [[Design-Decisions#404 vs 403 for cross-org access]].

> [!warning] Commit `docs/openapi.json` after handler changes
> CI runs a drift check. If you change any handler signature, regenerate and commit the spec.

---

## Related notes

- [[Architecture]] — understanding the structure before adding to it
- [[Testing-Strategy]] — how to write tests for your new code
- [[Authorization]] — how to add and enforce permissions
- [[Audit-System]] — how to emit audit events
- [[CI-and-Deployment]] — what CI checks will run on your PR
