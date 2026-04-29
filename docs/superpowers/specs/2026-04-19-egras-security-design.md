# egras Plan 2b — Security Domain & Deferred Tenants Use Cases (Design)

**Date:** 2026-04-19
**Status:** Approved (design), plan-to-be-written
**Prior work:**
- `docs/superpowers/plans/2026-04-18-egras-foundation-plan-1.md` — Plan 1 (foundation).
- `docs/superpowers/specs/2026-04-19-egras-tenants-vertical-slice-design.md` + `docs/superpowers/plans/2026-04-19-egras-tenants-plan-2a.md` — Plan 2a (tenants vertical slice, 4/6 use cases), merged to `main` via PR #1.
**Authoritative spec:** `knowledge/specs/2026-04-18-egras-rust-seed-design.md` — §6 (auth), §7 (audit), §8 (API surface), §5 (schema). Treat those sections as the contract; deviations are called out explicitly in §2 of this design.
**Successor:** Plan 3 — permission-denial audit emission, `audit/interface.rs`, `seed-admin` CLI, full E2E flows from spec §11.4.

## 1. Scope

Plan 2b ships two vertical slices:

1. **Two deferred tenants use cases** left over from Plan 2a.
2. **The full security domain** — all seven use cases of spec §6.

Plus: `AppState` + `MockAppStateBuilder` extensions; one E2E smoke; SQL-only bootstrap fixture for tests.

### 1.1 Use cases

| # | Domain | Use case | Spec ref | Route |
|---|---|---|---|---|
| 1 | tenants | `add_user_to_organisation` | §6.4 companion, §8.2 | `POST /api/v1/tenants/organisations/:id/members` |
| 2 | tenants | `remove_user_from_organisation` | §8.2 (last-owner → 409) | `DELETE /api/v1/tenants/organisations/:id/members/:user_id` |
| 3 | security | `register_user` | §6.4 | `POST /api/v1/security/register` |
| 4 | security | `login` | §6.2 | `POST /api/v1/security/login` |
| 5 | security | `logout` | §6.5 | `POST /api/v1/security/logout` |
| 6 | security | `change_password` | §6.6 | `POST /api/v1/security/change-password` |
| 7 | security | `switch_org` | §6.3 | `POST /api/v1/security/switch-org` |
| 8 | security | `password_reset_request` | §6.7 | `POST /api/v1/security/password-reset-request` |
| 9 | security | `password_reset_confirm` | §6.7 | `POST /api/v1/security/password-reset-confirm` |

### 1.2 Out of scope (remain Plan 3)

- Permission-denial audit emission from `RequirePermission` extractor (`permission.denied` events — spec §7.4).
- `audit/interface.rs` — `POST /api/v1/audit/list-audit-events` HTTP handler (service exists; binding does not).
- `seed-admin` CLI binary (spec §10).
- `dump-openapi` drift-check CI step beyond what Plan 2a already has.
- The other five E2E flows from spec §11.4.

### 1.3 Non-goals

- No new migrations. All required tables (`users`, `organisations`, `roles`, `user_organisation_roles`, `password_reset_tokens`, `audit_events`) exist from Plan 1.
- No new top-level dependencies in `Cargo.toml`. `argon2`, `jsonwebtoken`, `async-trait`, `mockall`, `validator` are all present.
- No changes to `auth/jwt.rs`, `auth/middleware.rs`, `auth/permissions.rs` — Plan 2b consumes them unchanged.
- No email integration for password reset (spec §6.7: log reset URL at INFO only).

### 1.4 Branching

`feat/security` branched from a freshly-pulled `main` (which includes the Plan 2a merge, `b2e01d1`). One PR at the end, merged to `main`. No stacked branches.

## 2. Accepted deviations from spec §8

- **Tenants routes are REST-style.** Plan 2a established `POST /organisations`, `GET /organisations/:id/members`, `POST /organisations/:id/memberships`. Plan 2b continues the pattern: `POST /organisations/:id/members` (add user), `DELETE /organisations/:id/members/:user_id` (remove user). Spec §8.2's action-style labels (`/add-user-to-organisation`, `/remove-user-from-organisation`) are treated as identifiers, not literal paths.
- **Security routes stay action-style per spec §8.1** — `/login`, `/register`, `/logout`, etc. Idiomatic for auth endpoints; no deviation.

## 3. Module Layout

Following Plan 2a's convention: one file per use case, trait + concrete impl, tests mirrored under `tests/`.

### 3.1 New files under `src/`

```
src/security/
├── mod.rs                                      (re-exports)
├── model.rs                                    (User, NewUser, PasswordResetToken, MembershipView)
├── password.rs                                 (argon2id hash/verify/needs_rehash)
├── persistence/
│   ├── mod.rs                                  (UserRepository, TokenRepository traits)
│   ├── user_repository_pg.rs
│   └── token_repository_pg.rs
├── service/
│   ├── mod.rs
│   ├── register_user.rs
│   ├── login.rs
│   ├── logout.rs
│   ├── change_password.rs
│   ├── switch_org.rs
│   ├── password_reset_request.rs
│   └── password_reset_confirm.rs
└── interface.rs                                (DTOs + handlers + router())

src/tenants/service/
├── add_user_to_organisation.rs                 (NEW)
└── remove_user_from_organisation.rs            (NEW)
```

### 3.2 Modified existing files

- `src/app_state.rs` — add `Arc<dyn …>` slot for each of the 9 new service traits plus 2 new security repo slots.
- `src/lib.rs::build_app` — instantiate services, register `security::interface::router()` under `/api/v1/security`, add two new routes to `tenants::interface::router()`.
- `src/testing.rs` — fluent setters on `MockAppStateBuilder` for every new service + repo slot.
- `src/tenants/interface.rs` — two new handlers (`post_add_user_to_organisation`, `delete_remove_user_from_organisation`).
- `src/tenants/persistence/organisation_repository_pg.rs` — extended with `add_member`, `remove_member`, `count_owners_for_update`.

### 3.3 Membership persistence

`add_user_to_organisation` / `remove_user_from_organisation` write to `user_organisation_roles`. The existing `OrganisationRepository` trait is **extended** with the required write methods (not a new `MembershipRepository` trait) — aggregate-root thinking, fewer traits, same sqlx struct.

## 4. Tenants Deferred Use Cases

### 4.1 `add_user_to_organisation`

**Trait:**

```rust
#[async_trait]
pub trait AddUserToOrganisation: Send + Sync {
    async fn execute(
        &self,
        caller: &Claims,
        caller_perms: &PermissionSet,
        input: AddMemberInput,
    ) -> Result<(), AppError>;
}

pub struct AddMemberInput {
    pub organisation_id: Uuid,
    pub user_id: Uuid,
    pub role_code: RoleCode,
}
```

**Flow:**

1. `authorise_org(caller, caller_perms, "tenants.members.add", organisation_id)` — 403 on miss, 404 on org-not-found per spec §3.5 rules Plan 2a already codifies.
2. Verify target user exists → 404 `resource.not_found` if missing.
3. Verify org exists → 404.
4. Insert `user_organisation_roles (user_id, organisation_id, role_id)` via pg repo. `ON CONFLICT (user_id, organisation_id, role_id) DO NOTHING` — idempotent.
5. `AuditRecorder::record(organisation.member_added { target_user_id, role_code, actor_user_id, actor_organisation_id })`.
6. `204 No Content`.

**Handler:** `POST /organisations/:id/members` — body `{ user_id, role_code }`. Path `:id` is the target org; the caller's JWT `org` need not match (authorisation is against the path-provided org).

### 4.2 `remove_user_from_organisation`

**Trait:**

```rust
#[async_trait]
pub trait RemoveUserFromOrganisation: Send + Sync {
    async fn execute(
        &self,
        caller: &Claims,
        caller_perms: &PermissionSet,
        organisation_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), AppError>;
}
```

**Flow:** All steps run in a single sqlx transaction.

1. `authorise_org(caller, caller_perms, "tenants.members.remove", organisation_id)`.
2. **Last-owner check with `FOR UPDATE` lock**:
   - `SELECT user_id FROM user_organisation_roles WHERE organisation_id = :org AND role_id = <org_owner_role_id> FOR UPDATE` — locks owner rows until commit.
   - If the target user holds `org_owner` in this org AND the locked result set has exactly one row (the target) → return `409 conflict.last_owner_cannot_be_removed`.
3. Delete all role rows for `(user_id, organisation_id)`. If zero rows deleted → `404 resource.not_found`.
4. Commit.
5. `AuditRecorder::record(organisation.member_removed { target_user_id, actor_user_id, actor_organisation_id })`.
6. `204`.

**Handler:** `DELETE /organisations/:id/members/:user_id`.

**Concurrency:** The `FOR UPDATE` lock serialises concurrent removers — without it, two concurrent deletes could each see `count=1` under `READ COMMITTED` and both succeed, leaving zero owners.

## 5. Security Domain — Cross-Cutting Design Decisions

Spec §6 covers flow-by-flow semantics; this section locks in choices that span multiple use cases.

### 5.1 argon2id configuration

```rust
// src/security/password.rs
use argon2::{Algorithm, Argon2, Params, Version};

pub fn hasher() -> Argon2<'static> {
    // OWASP 2024 baseline: m=19 MiB, t=2, p=1, 32-byte output
    let params = Params::new(19 * 1024, 2, 1, Some(32)).expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(plain: &str) -> Result<String, AppError>;
pub fn verify_password(plain: &str, stored: &str) -> Result<bool, AppError>;
pub fn needs_rehash(stored: &str) -> bool;  // compares parsed params to current baseline
```

Constants are hard-coded, not configurable — deliberate seed-grade choice. `needs_rehash` drives spec §6.2 step 7's opportunistic rehash on login.

### 5.2 JWT — no changes

`src/auth/jwt.rs` from Plan 1 has `encode_access_token` / `decode_access_token`. Plan 2b adds **no changes** to the JWT format or signing. Values (`jwt_secret`, `jwt_issuer`, `jwt_ttl_secs`) come from `AppConfig` via `AppState`, already wired by Plan 2a.

### 5.3 Password reset rate limiting

```rust
const MAX_PENDING_TOKENS_PER_USER: i64 = 3;

// SELECT count(*) FROM password_reset_tokens
// WHERE user_id = $1 AND consumed_at IS NULL AND expires_at > now();
```

If `count >= 3`, the service returns `Ok(None)` — no token issued. Handler still returns `204` (spec §6.7: no user enumeration). Audit event `password.reset_requested` is emitted only when a token IS issued, reducing log noise.

### 5.4 Bootstrap fixture (tests only)

New file: `tests/support/bootstrap.rs`, exported from existing `tests/support/mod.rs`.

```rust
pub struct BootstrapAdmin {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub access_token: String,
}

/// Seed one org + one user holding users.manage_all, and mint a JWT for them.
pub async fn seed_bootstrap_admin(pool: &PgPool, cfg: &AppConfig) -> BootstrapAdmin;
```

Inserts via raw SQL against the test pool. Bypasses the service layer entirely. Does **not** touch production code paths — production still has no bootstrap route until Plan 3's `seed-admin` CLI.

### 5.5 Opportunistic rehash on login (spec §6.2 step 7)

After a successful `verify_password`, `Login` service checks `needs_rehash(stored)`. If true:

- Rehash `plain` with current params.
- `UPDATE users SET password_hash = $new WHERE id = $user_id AND password_hash = $old` — compare-and-swap guard so concurrent rehashes don't clobber.
- Failure to update (zero rows affected) is logged at WARN but does not fail the login. Degraded, not broken.

### 5.6 Tokens table

Plan 1 ships the `password_reset_tokens` table. Columns: `id`, `user_id`, `token_hash` (sha256 hex), `expires_at`, `consumed_at NULLABLE`, `created_at`. `token_hash` is indexed for lookup. **No new migrations in Plan 2b.**

## 6. AppState & Testing Extensions

### 6.1 `AppState` additions (`src/app_state.rs`)

```rust
pub struct AppState {
    // … existing fields (pool, config, audit recorder, Plan 2a tenants services) …

    // Tenants — new
    pub add_user_to_organisation: Arc<dyn AddUserToOrganisation>,
    pub remove_user_from_organisation: Arc<dyn RemoveUserFromOrganisation>,

    // Security services
    pub register_user: Arc<dyn RegisterUser>,
    pub login: Arc<dyn Login>,
    pub logout: Arc<dyn Logout>,
    pub change_password: Arc<dyn ChangePassword>,
    pub switch_org: Arc<dyn SwitchOrg>,
    pub password_reset_request: Arc<dyn PasswordResetRequest>,
    pub password_reset_confirm: Arc<dyn PasswordResetConfirm>,

    // Security persistence (exposed for test fixtures + seed helpers)
    pub user_repository: Arc<dyn UserRepository>,
    pub token_repository: Arc<dyn TokenRepository>,
}
```

### 6.2 `build_app` wiring (`src/lib.rs`)

```rust
let user_repo:  Arc<dyn UserRepository>  = Arc::new(UserRepositoryPg::new(pool.clone()));
let token_repo: Arc<dyn TokenRepository> = Arc::new(TokenRepositoryPg::new(pool.clone()));

let register        = Arc::new(RegisterUserImpl::new(user_repo.clone(), org_repo.clone(), role_repo.clone(), audit.clone()));
let login           = Arc::new(LoginImpl::new(user_repo.clone(), org_repo.clone(), role_repo.clone(), audit.clone(), cfg.clone()));
let logout          = Arc::new(LogoutImpl::new(audit.clone()));
let change_password = Arc::new(ChangePasswordImpl::new(user_repo.clone(), audit.clone()));
let switch_org      = Arc::new(SwitchOrgImpl::new(org_repo.clone(), role_repo.clone(), audit.clone(), cfg.clone()));
let pwd_request     = Arc::new(PasswordResetRequestImpl::new(user_repo.clone(), token_repo.clone(), audit.clone(), cfg.clone()));
let pwd_confirm     = Arc::new(PasswordResetConfirmImpl::new(user_repo.clone(), token_repo.clone(), audit.clone()));

let add_user    = Arc::new(AddUserToOrganisationImpl::new(org_repo.clone(), user_repo.clone(), role_repo.clone(), audit.clone()));
let remove_user = Arc::new(RemoveUserFromOrganisationImpl::new(org_repo.clone(), audit.clone()));

let router = Router::new()
    .nest("/api/v1/tenants",  tenants::interface::router())
    .nest("/api/v1/security", security::interface::router())
    .route("/health", …)
    .route("/ready", …)
    .layer(auth_layer)
    .layer(trace_layer)
    .layer(cors_layer)
    .with_state(state);
```

No changes to the existing audit worker or graceful-shutdown wiring.

### 6.3 `MockAppStateBuilder` additions (`src/testing.rs`)

Eleven fluent setters (9 services + 2 repos) mirroring Plan 2a's pattern:

```rust
impl MockAppStateBuilder {
    // 9 services
    pub fn with_add_user_to_organisation(mut self, s: Arc<dyn AddUserToOrganisation>) -> Self { … }
    pub fn with_remove_user_from_organisation(mut self, s: Arc<dyn RemoveUserFromOrganisation>) -> Self { … }
    pub fn with_register_user(mut self, s: Arc<dyn RegisterUser>) -> Self { … }
    pub fn with_login(mut self, s: Arc<dyn Login>) -> Self { … }
    pub fn with_logout(mut self, s: Arc<dyn Logout>) -> Self { … }
    pub fn with_change_password(mut self, s: Arc<dyn ChangePassword>) -> Self { … }
    pub fn with_switch_org(mut self, s: Arc<dyn SwitchOrg>) -> Self { … }
    pub fn with_password_reset_request(mut self, s: Arc<dyn PasswordResetRequest>) -> Self { … }
    pub fn with_password_reset_confirm(mut self, s: Arc<dyn PasswordResetConfirm>) -> Self { … }
    // 2 repos
    pub fn with_user_repository(mut self, r: Arc<dyn UserRepository>) -> Self { … }
    pub fn with_token_repository(mut self, r: Arc<dyn TokenRepository>) -> Self { … }
}
```

Each slot defaults to a `mockall` auto-mock that panics on any call ("unexpected call") — Plan 2a's established pattern. Tests override only the slots their specific case exercises.

### 6.4 `TestApp` — thin additions

- `tests/support/bootstrap.rs::seed_bootstrap_admin` — see §5.4.
- `TestApp::login_as(user_id, org_id) -> String` — thin wrapper around the existing `mint_jwt`, for ergonomics in register/login tests.

## 7. Test Strategy

Three layers, each answering a different question. All run on `cargo test`.

### 7.1 Persistence layer — DB-backed

**Files:**

- `tests/security/persistence/user_repository_pg_test.rs`
- `tests/security/persistence/token_repository_pg_test.rs`
- `tests/tenants/persistence/organisation_repository_pg_test.rs` — **extended** with tests for `add_member`, `remove_member`, `count_owners_for_update`.

**Scope:** Each public repo method gets direct-call tests against the shared `TestPool` from Plan 1/2a. Assertions on DB state and returned values. No mocks.

**Coverage:**

- `UserRepository`: create, find-by-id, find-by-username, find-by-email (citext, case-insensitive), update-password-hash with CAS guard.
- `TokenRepository`: insert, find-by-hash, mark-consumed, count-pending-for-user, expiry filter.
- `OrganisationRepository` additions: add-member idempotency (ON CONFLICT), remove-all-roles-for-member, last-owner count with `FOR UPDATE`.

### 7.2 Service layer — mocked repos

One `_test.rs` file per use case (9 files). Services constructed with `mockall` traits. Each test exercises exactly one business rule.

| Use case | Cases covered |
|---|---|
| `register_user` | happy path; caller lacks both permissions → 403; target org missing → 404; role_code not assignable → 403; duplicate username → 409 `conflict.user_exists` |
| `login` | happy path (JWT default org = oldest membership); invalid password → 401 (same shape as user-not-found); zero memberships → 403 `user.no_organisation`; opportunistic rehash fires on outdated hash |
| `logout` | happy path emits audit event with `jti` |
| `change_password` | happy path; current password wrong → 401 |
| `switch_org` | happy path; caller not a member → 403; target org missing → 403 (not 404 — spec §6.3 says "permission.denied") |
| `password_reset_request` | email exists + under quota → token issued + audit + log; email exists + at quota → `Ok(None)`, no audit, no log; email missing → `Ok(None)`, no audit; all return paths map to handler 204 |
| `password_reset_confirm` | happy path; token hash not found → 400; token expired → 400; already consumed → 400; success marks consumed + emits audit |
| `add_user_to_organisation` | happy path; duplicate (same role) idempotent; target user missing → 404; caller lacks `tenants.members.add` → 403 |
| `remove_user_from_organisation` | happy path; last owner → 409 `conflict.last_owner_cannot_be_removed`; target not a member → 404; caller lacks `tenants.members.remove` → 403 |

**Audit assertions:** every state-change test asserts the audit recorder mock received exactly one expected event. Reuse Plan 2a's `BlockingAuditRecorder` or a fresh `MockAuditRecorder`.

### 7.3 Interface layer — HTTP, mocked services

One `_test.rs` file per endpoint (9 files). Boot `TestApp` with `MockAppStateBuilder` where only the service under test has a real mock; everything else stays in panic-on-call default. Assertions:

- HTTP status codes + Problem+JSON error shape (`type`, `title`, `status`, `code`, `request_id`).
- DTO deserialisation (400 on malformed body).
- Auth ordering: 401 before 403 before 400 (Plan 2a's `Perm` extractor precedent).
- Route parameters (`:id`, `:user_id`) parsed correctly.
- `X-Request-Id` header echoed in every response.

### 7.4 E2E smoke — one flow

**File:** `tests/e2e/register_login_smoke.rs`.

**Flow:**

1. `seed_bootstrap_admin(pool, cfg)` → admin user, admin org A (admin is `org_owner`), admin JWT. The fixture also seeds a second empty org B with admin as `org_owner`.
2. `POST /api/v1/security/register` with admin JWT, `invited_to_organisation_id = A` → creates user B with `org_member` role in org A. Assert `201` + returned `user_id`.
3. `POST /api/v1/tenants/organisations/:B/members` with admin JWT, body `{ user_id: B, role_code: org_member }` → adds user B to org B as `org_member`. Assert `204`.
4. `POST /api/v1/security/login` as user B with plain credentials → assert `200` + JWT scoped to the oldest membership (org A by `created_at`) + memberships list contains both orgs.
5. `POST /api/v1/security/switch-org` with user B's JWT, body `{ organisation_id: B }` → assert `200` + new JWT scoped to org B.

Runs against the real Postgres container (TestPool). No mocks. Verifies the entire stack: argon2, JWT issuance, sqlx, audit worker, auth middleware, permission loading.

### 7.5 Out of test scope (Plan 3)

- The other five E2E flows from spec §11.4.
- Tracing-output assertions for the password-reset log line (too brittle; service-level test covers the "token issued" side effect by return value).

## 8. Known Limitations (accepted, documented)

- **`password_reset_request` rate limit is user-scoped, not email-scoped.** Attackers supplying unknown emails learn nothing (service always returns `Ok(None)`), so global per-email throttling is unnecessary at this grade.
- **`logout` does not revoke the JWT.** Spec-mandated stateless seed. Token remains valid until `exp`.
- **`switch_org` issues a new JWT without invalidating the old one** (spec §6.3). Same caveat as logout.
- **Opportunistic rehash CAS guard is advisory.** Two concurrent logins with an outdated hash: one `UPDATE` succeeds, the other no-ops. Logged WARN.
- **`add_user_to_organisation` is additive, not replace.** A user can hold multiple roles in one org per the §5 schema. If the caller wants replace semantics, they use `assign_role` with `revoke_existing = true` (Plan 2a endpoint).

## 9. Plan-3 Dependencies

- Plan 3 extends the E2E suite; no refactoring of Plan 2b modules expected.
- Plan 3's `seed-admin` CLI may call `RegisterUserImpl` directly (outside Axum). For that to work, `RegisterUser::execute` keeps accepting `caller: &Claims` — the CLI constructs a synthetic admin `Claims` for bootstrap. **Design note:** do **not** refactor the trait to take a simpler caller type.

## 10. Acceptance

Plan 2b is "done" when:

1. All 9 services + 2 repos + 1 HTTP router are implemented.
2. All persistence / service / interface tests pass against Postgres in CI.
3. `tests/e2e/register_login_smoke.rs` passes.
4. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo build --release` are clean.
5. `feat/security` branches cleanly from `main` (after fetch/pull) and the PR merges green.
6. No new migrations, no new top-level dependencies, no changes to `auth/*` files.
