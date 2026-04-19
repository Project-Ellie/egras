# egras Plan 2a — Tenants Vertical Slice (Design)

**Date:** 2026-04-19
**Status:** Approved (design), plan-to-be-written
**Prior work:** `docs/superpowers/plans/2026-04-18-egras-foundation-plan-1.md` (Plan 1 — foundation) landed on `feat/foundation`.
**Successor:** Plan 2b will cover cross-organisation user lifecycle (add-user, remove-user, password resets) and operator-only security endpoints (roles/permissions CRUD, user provisioning).

## 1. Scope

Plan 2a ships a complete **vertical slice** of the tenants domain: model + persistence + service + HTTP handlers + tests, for four endpoints mounted under `/api/v1/tenants/`.

### Endpoints in scope

1. `POST   /api/v1/tenants/organisations`                  — create a new organisation (and optionally seed the caller as initial owner)
2. `GET    /api/v1/tenants/me/organisations`               — list organisations the authenticated caller is a member of
3. `GET    /api/v1/tenants/organisations/{id}/members`     — list members of an organisation
4. `POST   /api/v1/tenants/organisations/{id}/memberships` — assign a role to a user in an organisation

### Deferred to Plan 2b

- `POST /api/v1/tenants/organisations/{id}/users`               — invite/add a user to an organisation
- `DELETE /api/v1/tenants/organisations/{id}/users/{user_id}`   — remove a user from an organisation
- Operator-only security endpoints (roles CRUD, permissions CRUD, user provisioning)

### Why this subset

The four endpoints are a minimal self-contained loop: an operator can create an org, list orgs, assign a role, and list members. User lifecycle (add-user, remove-user) is intentionally deferred — it depends on Plan 2b's user-management endpoints, and including it here would re-introduce the chicken-and-egg problem where tests need a bootstrapped user to assign a role to. For Plan 2a, test users are seeded via SQL fixtures (see §4).

### Non-goals

- No new migrations. Migrations 0001–0006 on `feat/foundation` cover all tables Plan 2a needs.
- No soft deletes, no updates to organisation metadata, no org hierarchy. These can land in later plans if/when required.
- No operator-only bypass UI — the `*.manage_all` permission handling is enforced at the service layer per §3.5 of the foundation plan, but no dedicated endpoint exposes it.

## 2. Architecture

### Module layout

```
src/tenants/
  mod.rs
  model.rs                         # domain types
  persistence/
    mod.rs
    organisation_repository.rs     # trait
    organisation_repository_pg.rs  # Postgres impl
    role_repository.rs             # trait
    role_repository_pg.rs          # Postgres impl
  service/
    mod.rs
    create_organisation.rs         # one use case per file (per foundation §3.3)
    list_my_organisations.rs
    list_organisation_members.rs
    assign_role.rs
  interface.rs                     # axum handlers + router builder
```

`AppState` gains `organisations: Arc<dyn OrganisationRepository>` and `roles: Arc<dyn RoleRepository>`. The protected router in `src/lib.rs` mounts `tenants::interface::router()` under `/api/v1/tenants`.

### Domain model (`src/tenants/model.rs`)

```rust
pub struct Organisation {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub is_operator: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Membership {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub role_id: Uuid,
    pub role_code: String,      // denormalised from roles.code for cheap listing
    pub created_at: DateTime<Utc>,
}

pub struct OrganisationSummary {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>, // the caller's roles in this org
}

pub struct MemberSummary {
    pub user_id: Uuid,
    pub username: String,  // users.username (users table has no display_name column)
    pub email: String,
    pub role_codes: Vec<String>,
}

pub struct OrganisationCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

pub struct MembershipCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
```

Role codes are `String`, not an enum — operators may define custom roles (migration 0005 seeds the built-in codes `operator_admin`, `org_admin`, `org_member`; custom codes come from Plan 2b).

### Persistence traits

```rust
#[async_trait]
pub trait OrganisationRepository: Send + Sync {
    async fn create(&self, name: &str, business: &str) -> Result<Organisation, DbError>;

    /// Create an organisation and assign `owner_role_code` to `creator_user_id` in one tx.
    async fn create_with_initial_owner(
        &self,
        name: &str,
        business: &str,
        creator_user_id: Uuid,
        owner_role_code: &str,
    ) -> Result<Organisation, DbError>;

    async fn list_for_user(
        &self,
        user_id: Uuid,
        after: Option<OrganisationCursor>,
        limit: u32,
    ) -> Result<Vec<OrganisationSummary>, DbError>;

    async fn list_members(
        &self,
        organisation_id: Uuid,
        after: Option<MembershipCursor>,
        limit: u32,
    ) -> Result<Vec<MemberSummary>, DbError>;
}

#[async_trait]
pub trait RoleRepository: Send + Sync {
    async fn find_by_code(&self, code: &str) -> Result<Option<Role>, DbError>;

    async fn assign(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), DbError>;
}
```

`Role` is a tiny helper struct `{ id: Uuid, code: String }` in `model.rs`.

### Service use cases

Each use case is its own file in `src/tenants/service/` (per foundation §3.3) and:

1. Takes `AppState` + `Claims` + request DTO, returns a response DTO or a typed service error.
2. Enforces permissions via `PermissionSet`; `*.manage_all` bypasses cross-org checks (foundation §3.5).
3. Returns **404** when a caller without `*.manage_all` touches a resource in an organisation they do not belong to (foundation §3.5).
4. Emits an `AuditEvent` via `AppState::audit_tx` on success (non-blocking in production; see §3 for tests).

Use case → permission mapping:

| Use case                   | Required permission       | Notes                                                     |
|----------------------------|---------------------------|-----------------------------------------------------------|
| `create_organisation`      | `tenants.create`          | Caller seeded as `org_owner` unless payload opts out.     |
| `list_my_organisations`    | (authenticated, no extra) | Scoped to caller's memberships.                           |
| `list_organisation_members`| `tenants.members.list`    | 404 on non-member without `tenants.manage_all`.           |
| `assign_role`              | `tenants.roles.assign`    | 404 on non-member without `tenants.manage_all`. Idempotent: re-assigning an existing `(user, org, role)` triple returns 200 (not 409).|

Permission codes are the actual codes defined in migration 0005 (`tenants.create`, `tenants.members.list`, `tenants.roles.assign`, `tenants.manage_all`). Role codes (`org_owner`, `org_admin`, `org_member`, `operator_admin`) likewise come from migration 0005.

### HTTP layer (`src/tenants/interface.rs`)

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/organisations", post(create_organisation))
        .route("/me/organisations", get(list_my_organisations))
        .route("/organisations/:id/members", get(list_organisation_members))
        .route("/organisations/:id/memberships", post(assign_role))
}
```

Request/response DTOs live at the top of `interface.rs` (Plan 1 convention). All errors flow through the existing RFC 7807 `ApiError` conversion.

### Error mapping

Service errors map to HTTP as follows (matching foundation §4):

Service errors re-use the foundation `AppError` enum (see `src/errors.rs`). Mapping:

| Condition                       | `AppError` variant                   | HTTP | slug                          |
|---------------------------------|--------------------------------------|------|-------------------------------|
| Non-member w/o `manage_all`     | `NotFound { resource: "organisation" }` | 404  | `resource.not_found`          |
| Duplicate organisation name     | `Conflict { reason }`                | 409  | `resource.conflict`           |
| Unknown role code in payload    | `Validation { errors }`              | 400  | `validation.invalid_request`  |
| Unknown target user id          | `Validation { errors }`              | 400  | `validation.invalid_request`  |
| Missing permission              | `PermissionDenied { code }`          | 403  | `permission.denied`           |
| DB / other internal failure     | `Internal(anyhow::Error)`            | 500  | `internal.error`              |

## 3. Testing strategy (hybrid)

Two layers, both against real Postgres via `TestPool::fresh()`:

### 3.1 Service-level tests (`tests/tenants_service/`)

One file per use case. Each test:

1. Calls `TestPool::fresh()` to get a migrated empty DB.
2. Seeds fixtures via `tests/common/fixtures.rs` SQL helpers (see §4).
3. Builds a minimal `AppState` (mock `audit_tx` channel; real repos wired to the test pool).
4. Calls the use case function directly.
5. Asserts return value and DB side effects.

Service tests are cheap, fast, and prove the domain logic in isolation from axum plumbing.

### 3.2 HTTP E2E tests (`tests/tenants_http/`)

One file per endpoint. Each file exercises the full stack (router + AuthLayer + handler + service + real pg) via `TestApp::spawn()`. Audit assertions use `BlockingAuditRecorder` (foundation Task 19) for deterministic ordering.

Each endpoint file covers a **4-case matrix**:

| Case                | Assertion                                                  |
|---------------------|------------------------------------------------------------|
| Unauthenticated     | 401 JSON with RFC 7807 body                                |
| Missing permission  | 403 JSON                                                   |
| Happy path          | 2xx, correct body, audit event emitted                     |
| Domain failure      | 404 / 409 / 400 depending on endpoint (see error map §2)   |

### 3.3 Fixtures (`tests/common/fixtures.rs`)

Plan 2a adds SQL-level seed helpers (no repo calls) so service tests can set up state without depending on code under test:

```rust
pub async fn seed_user(pool: &PgPool, email: &str) -> Uuid { /* ... */ }
pub async fn seed_operator_org(pool: &PgPool) -> Uuid { /* ... */ }  // idempotent
pub async fn seed_org(pool: &PgPool, name: &str, business: &str) -> Uuid { /* ... */ }
pub async fn grant_role(pool: &PgPool, user_id: Uuid, org_id: Uuid, role_code: &str);
pub async fn issue_jwt_for(user_id: Uuid, role_codes: &[&str]) -> String;
```

`issue_jwt_for` uses the test JWT secret wired into `TestApp`.

## 4. Plan structure & execution

### Plan style

The implementation plan will be written in **intent + contract + fixtures** style (not verbatim code). Each task specifies:

- **Intent** — what we're building and why it matters.
- **Contract** — exact trait / DTO / route signatures the implementer must match.
- **Fixtures** — test helpers and seed data needed.
- **Acceptance** — cargo test invocation and what must pass.

The implementer subagent is expected to write idiomatic Rust that satisfies the contract; the plan does not dictate line-level code.

### Task breakdown (7 tasks)

| # | Task                                              | Scope                                                                                        |
|---|---------------------------------------------------|----------------------------------------------------------------------------------------------|
| 1 | Tenants model + module skeleton                   | `src/tenants/{mod.rs,model.rs}`; empty `persistence/` and `service/` submodules.              |
| 2 | Persistence: OrganisationRepository + RoleRepository | Traits + Postgres impls; unit tests against `TestPool::fresh()`.                            |
| 3 | AppState extension + fixtures helpers             | Add repo fields to `AppState`; add `MockAppStateBuilder` hooks; write `tests/common/fixtures.rs`. |
| 4 | Vertical: `create_organisation`                   | service + handler + DTOs + service tests + HTTP 4-case matrix.                               |
| 5 | Vertical: `list_my_organisations`                 | service + handler + DTOs + service tests + HTTP 4-case matrix.                               |
| 6 | Vertical: `list_organisation_members`             | service + handler + DTOs + service tests + HTTP 4-case matrix.                               |
| 7 | Vertical: `assign_role`                           | service + handler + DTOs + service tests + HTTP 4-case matrix.                               |

Tasks 1–3 lay the shared scaffold; tasks 4–7 are independent vertical slices that can ship individually. Each task must end with `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` green.

### Worktree & branching

- Create a new git worktree `feat/tenants` branched from `feat/foundation` (Plan 1's branch; not yet merged to `main`).
- Execute Plan 2a via superpowers:subagent-driven-development on `feat/tenants`.
- CI workflow from Plan 1 already exercises the shared-pg harness; no workflow changes needed.

### Acceptance

Plan 2a is complete when:

- All four endpoints are reachable in `docker compose up` and return sensible responses for the happy path.
- `cargo test --all-features` is green on `feat/tenants` (both service-level and HTTP E2E suites).
- `cargo fmt --check` and `cargo clippy -D warnings` pass.
- CI green on `feat/tenants`.
- Every successful write emits an audit event (verified in HTTP tests via `BlockingAuditRecorder`).
- Cross-organisation 404 behaviour is exercised by at least one failing-case test per relevant endpoint.

## 5. Out of scope (reiterated)

- Add-user / remove-user endpoints (→ Plan 2b).
- Roles/permissions CRUD (→ Plan 2b).
- User provisioning endpoints (→ Plan 2b).
- Any new database migrations.
- Operator bypass UI or admin-console endpoints.
