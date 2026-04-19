# egras Plan 2a — Tenants Vertical Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the first vertical slice of the tenants domain — create-organisation, list-my-organisations, list-organisation-members, and assign-role — end-to-end on the foundation scaffold.

**Architecture:** Per the design at `docs/superpowers/specs/2026-04-19-egras-tenants-vertical-slice-design.md`. Four endpoints mounted under `/api/v1/tenants` behind `AuthLayer`. Two persistence traits (`OrganisationRepository`, `RoleRepository`), one use case per file under `src/tenants/service/`, and HTTP handlers in `src/tenants/interface.rs`. Hybrid tests: direct service-level tests against `TestPool::fresh()` plus HTTP E2E tests with a 4-case matrix (unauth / missing-perm / happy / domain-failure).

**Tech Stack:** Rust 2021, axum 0.7, sqlx 0.8 (Postgres), uuid v7, chrono, serde, async-trait, thiserror, tokio, tracing. No new dependencies. Uses the shared-pg test harness from Plan 1 (`TestPool`, `BlockingAuditRecorder`, `MockAppStateBuilder`, `TestApp::spawn`, `mint_jwt`).

**Branch:** Execute on `feat/tenants`, branched from `feat/foundation`.

---

## Prerequisites (one-time)

- [ ] **Create the worktree**

From the main egras checkout (not inside an existing worktree):

```bash
cd /Users/wgiersche/workspace/Project-Ellie/egras
git fetch --all
git worktree add -b feat/tenants .worktrees/feat-tenants feat/foundation
cd .worktrees/feat-tenants
```

Verify you are on the right branch and that Plan 1's commits are present:

```bash
git rev-parse --abbrev-ref HEAD    # → feat/tenants
git log --oneline -5               # → should include d2817f0 "fix(test): switch TestPool..."
cargo build                        # should succeed
```

- [ ] **Ensure the shared test Postgres is running**

```bash
docker ps --filter name=egras-test-pg --format '{{.Names}}'
# if empty:
docker run -d --name egras-test-pg \
  -e POSTGRES_USER=egras -e POSTGRES_PASSWORD=egras -e POSTGRES_DB=postgres \
  -p 15432:5432 postgres:16-alpine

# smoke:
psql postgres://egras:egras@127.0.0.1:15432/postgres -c 'SELECT 1'
```

- [ ] **Baseline**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

All three must pass before starting Task 1. If they do not, stop and investigate — the branch is not in a known-good state.

---

## Task 1: Domain model + module skeleton

**Files:**
- Create: `src/tenants/model.rs`
- Modify: `src/tenants/mod.rs`
- Create: `src/tenants/persistence/mod.rs` (empty-ish)
- Create: `src/tenants/service/mod.rs` (empty-ish)

**Intent:** Define the tenants domain types so subsequent tasks have concrete structs to import. No persistence or service code yet — this task compiles and is done.

- [ ] **Step 1: Write `src/tenants/model.rs`**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organisation {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub is_operator: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: Uuid,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub role_id: Uuid,
    pub role_code: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganisationSummary {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
    /// Carried through so the caller can build `OrganisationCursor` from a page row.
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganisationCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
```

- [ ] **Step 2: Update `src/tenants/mod.rs`**

Replace the placeholder content with:

```rust
pub mod model;
pub mod persistence;
pub mod service;
```

(`interface` is added in Task 4.)

- [ ] **Step 3: Create empty submodule files**

```bash
cat > src/tenants/persistence/mod.rs <<'EOF'
// Traits and Postgres implementations land in Task 2.
EOF
cat > src/tenants/service/mod.rs <<'EOF'
// Use cases land in Tasks 4–7 (one file per use case).
EOF
```

- [ ] **Step 4: Verify build**

```bash
cargo build
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/tenants/
git commit -m "feat(tenants): add domain model + module skeleton"
```

---

## Task 2: Persistence — OrganisationRepository + RoleRepository

**Files:**
- Create: `src/tenants/persistence/organisation_repository.rs`
- Create: `src/tenants/persistence/organisation_repository_pg.rs`
- Create: `src/tenants/persistence/role_repository.rs`
- Create: `src/tenants/persistence/role_repository_pg.rs`
- Modify: `src/tenants/persistence/mod.rs`
- Create: `tests/tenants_persistence_test.rs`

**Intent:** Two trait+impl pairs. `OrganisationRepository` owns org creation (including the "create-and-seed-owner" transaction) plus org/member listings. `RoleRepository` finds roles by code and assigns a role in a `(user, org)` pair.

### 2.1 Trait contracts

- [ ] **Step 1: Write `src/tenants/persistence/organisation_repository.rs`**

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::{
    MemberSummary, MembershipCursor, Organisation, OrganisationCursor, OrganisationSummary,
};

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("duplicate organisation name: {0}")]
    DuplicateName(String),
    #[error("unknown role code: {0}")]
    UnknownRoleCode(String),
    #[error("unknown user: {0}")]
    UnknownUser(Uuid),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait OrganisationRepository: Send + Sync + 'static {
    async fn create(&self, name: &str, business: &str) -> Result<Organisation, RepoError>;

    /// Create an organisation and assign `owner_role_code` to `creator_user_id`
    /// inside a single transaction. Used by `create_organisation`.
    async fn create_with_initial_owner(
        &self,
        name: &str,
        business: &str,
        creator_user_id: Uuid,
        owner_role_code: &str,
    ) -> Result<Organisation, RepoError>;

    async fn list_for_user(
        &self,
        user_id: Uuid,
        after: Option<OrganisationCursor>,
        limit: u32,
    ) -> Result<Vec<OrganisationSummary>, RepoError>;

    async fn list_members(
        &self,
        organisation_id: Uuid,
        after: Option<MembershipCursor>,
        limit: u32,
    ) -> Result<Vec<MemberSummary>, RepoError>;

    /// Returns true iff `(user_id, organisation_id)` has at least one role row.
    /// Used by the cross-org rule in service layer.
    async fn is_member(&self, user_id: Uuid, organisation_id: Uuid) -> Result<bool, RepoError>;
}
```

- [ ] **Step 2: Write `src/tenants/persistence/role_repository.rs`**

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::Role;
use crate::tenants::persistence::organisation_repository::RepoError;

#[async_trait]
pub trait RoleRepository: Send + Sync + 'static {
    async fn find_by_code(&self, code: &str) -> Result<Option<Role>, RepoError>;

    /// Idempotent: a row already matching `(user, org, role)` is a no-op, not a
    /// conflict. `UnknownUser` / `UnknownRoleCode` map to `RepoError::UnknownUser`
    /// / `RepoError::UnknownRoleCode` rather than a raw FK violation.
    async fn assign(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), RepoError>;
}
```

- [ ] **Step 3: Update `src/tenants/persistence/mod.rs`**

```rust
pub mod organisation_repository;
pub mod organisation_repository_pg;
pub mod role_repository;
pub mod role_repository_pg;

pub use organisation_repository::{OrganisationRepository, RepoError};
pub use organisation_repository_pg::OrganisationRepositoryPg;
pub use role_repository::RoleRepository;
pub use role_repository_pg::RoleRepositoryPg;
```

### 2.2 Postgres implementations

- [ ] **Step 4: Write `src/tenants/persistence/organisation_repository_pg.rs`**

Implementation guidance (write the Rust; do not copy-paste SQL into the plan):

- Constructor: `pub fn new(pool: PgPool) -> Self { Self { pool } }`.
- `create`: `INSERT INTO organisations (id, name, business, is_operator) VALUES ($1, $2, $3, FALSE) RETURNING id, name, business, is_operator, created_at, updated_at`. Map Postgres `23505` (unique violation) on `organisations_name_key` → `RepoError::DuplicateName(name)`.
- `create_with_initial_owner`: open a `pool.begin()` transaction. Insert the organisation (same SQL), then resolve `owner_role_code` via `SELECT id FROM roles WHERE code = $1`. If NULL → roll back, return `RepoError::UnknownRoleCode`. Otherwise insert `(creator_user_id, org.id, role_id)` into `user_organisation_roles`. If the creator id is missing → `RepoError::UnknownUser(creator_user_id)`. Commit.
- `list_for_user`: join `user_organisation_roles ↔ organisations ↔ roles`, group by org, aggregate role codes into a `TEXT[]`. Order by `(organisations.created_at DESC, organisations.id DESC)`. Cursor filter: `(created_at, id) < ($after_created_at, $after_id)`. Apply `LIMIT = limit`.
- `list_members`: join `user_organisation_roles ↔ users ↔ roles` for the given org. Group by user. Order by `(uor.created_at DESC, user_id DESC)`. Cursor predicate analogous to above. Return usernames + emails from `users`.
- `is_member`: `SELECT EXISTS(SELECT 1 FROM user_organisation_roles WHERE user_id = $1 AND organisation_id = $2)`.

All queries use `sqlx::query_as!` where stable or explicit struct mapping with `FromRow` otherwise. Use `Uuid::now_v7()` for new organisation ids.

- [ ] **Step 5: Write `src/tenants/persistence/role_repository_pg.rs`**

- `find_by_code`: `SELECT id, code FROM roles WHERE code = $1`. Use `fetch_optional`.
- `assign`: `INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING`. Map FK violations: `23503` on `fk_uor_user` → `UnknownUser(user_id)`; on `fk_uor_role` → `UnknownRoleCode("<role_id>")` (the service resolves the human code upstream, so this branch is rare — fall back to `Db(err)` if the constraint name is unknown).

### 2.3 Persistence tests

- [ ] **Step 6: Write `tests/tenants_persistence_test.rs`**

```rust
use egras::tenants::model::{MembershipCursor, OrganisationCursor};
use egras::tenants::persistence::{
    OrganisationRepository, OrganisationRepositoryPg, RepoError, RoleRepository, RoleRepositoryPg,
};
use egras::testing::TestPool;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'x')",
    )
    .bind(id)
    .bind(username)
    .bind(format!("{username}@test"))
    .execute(pool)
    .await
    .expect("seed user");
    id
}

#[tokio::test]
async fn create_returns_organisation_with_non_operator_flag() {
    let pool = TestPool::fresh().await.pool;
    let repo = OrganisationRepositoryPg::new(pool);

    let org = repo.create("acme", "retail").await.unwrap();
    assert_eq!(org.name, "acme");
    assert_eq!(org.business, "retail");
    assert!(!org.is_operator);
}

#[tokio::test]
async fn create_duplicate_name_maps_to_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let repo = OrganisationRepositoryPg::new(pool);
    repo.create("acme", "retail").await.unwrap();

    let err = repo.create("acme", "media").await.unwrap_err();
    assert!(matches!(err, RepoError::DuplicateName(n) if n == "acme"));
}

#[tokio::test]
async fn create_with_initial_owner_assigns_role_in_one_tx() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let org = orgs
        .create_with_initial_owner("acme", "retail", user, "org_owner")
        .await
        .unwrap();

    let members = orgs
        .list_members(org.id, None, 50)
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].user_id, user);
    assert_eq!(members[0].role_codes, vec!["org_owner"]);
}

#[tokio::test]
async fn create_with_initial_owner_rolls_back_on_unknown_role() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let err = orgs
        .create_with_initial_owner("acme", "retail", user, "no_such_role")
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::UnknownRoleCode(_)));

    // Nothing landed.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organisations WHERE name = 'acme'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn list_for_user_is_scoped_and_paginated() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let other = seed_user(&pool, "bob").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    for name in ["a", "b", "c"] {
        orgs.create_with_initial_owner(name, "retail", user, "org_owner")
            .await
            .unwrap();
    }
    orgs.create_with_initial_owner("hidden", "retail", other, "org_owner")
        .await
        .unwrap();

    let page1 = orgs.list_for_user(user, None, 2).await.unwrap();
    assert_eq!(page1.len(), 2);

    let last = page1.last().unwrap();
    let cursor = OrganisationCursor {
        created_at: last.created_at,
        id: last.id,
    };

    let page2 = orgs.list_for_user(user, Some(cursor), 2).await.unwrap();
    assert_eq!(page2.len(), 1);
    assert!(!page2.iter().any(|o| o.name == "hidden"));
}

#[tokio::test]
async fn roles_find_by_code_returns_builtin() {
    let pool = TestPool::fresh().await.pool;
    let repo = RoleRepositoryPg::new(pool);

    let r = repo.find_by_code("org_owner").await.unwrap().unwrap();
    assert_eq!(r.code, "org_owner");

    assert!(repo.find_by_code("no_such").await.unwrap().is_none());
}

#[tokio::test]
async fn roles_assign_is_idempotent() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());
    let roles = RoleRepositoryPg::new(pool.clone());

    let org = orgs.create("acme", "retail").await.unwrap();
    let role = roles.find_by_code("org_member").await.unwrap().unwrap();

    roles.assign(user, org.id, role.id).await.unwrap();
    // second call must succeed (ON CONFLICT DO NOTHING)
    roles.assign(user, org.id, role.id).await.unwrap();

    let _members = orgs.list_members(org.id, None, 10).await.unwrap();
    // Exactly one member, one role code.
    assert!(_members.iter().all(|m| m.role_codes == vec!["org_member"]));
    assert_eq!(_members.len(), 1);
}

#[tokio::test]
async fn is_member_true_only_for_actual_members() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let org = orgs
        .create_with_initial_owner("acme", "retail", user, "org_owner")
        .await
        .unwrap();
    assert!(orgs.is_member(user, org.id).await.unwrap());
    let stranger = seed_user(&pool, "mallory").await;
    assert!(!orgs.is_member(stranger, org.id).await.unwrap());
}
```

Remove `MembershipCursor` from the `use` line if it is unused; do the same for `RepoError` if warnings appear.

- [ ] **Step 7: Run the failing tests**

```bash
cargo test --all-features --test tenants_persistence_test
```

Expected: compile errors pointing at `OrganisationSummary.created_at` (from the stub note) and any missing fields. Iterate: amend model, re-run. Once code compiles, expect all 7 tests to pass.

- [ ] **Step 8: Fmt + clippy**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

- [ ] **Step 9: Commit**

```bash
git add src/tenants/ tests/tenants_persistence_test.rs
git commit -m "feat(tenants): OrganisationRepository + RoleRepository with pg impls"
```

---

## Task 3: AppState extension + fixtures helpers

**Files:**
- Modify: `src/app_state.rs`
- Modify: `src/lib.rs` (build_app wiring)
- Modify: `src/testing.rs` (`MockAppStateBuilder` setters)
- Modify: `tests/common/fixtures.rs`
- Create: `tests/common/seed.rs`
- Modify: `tests/common/mod.rs`

**Intent:** Plumb the two repos into `AppState`, extend the test `MockAppStateBuilder` with setters, and add SQL-level seed helpers under `tests/common/seed.rs` so that service-level tests can set up state without calling code under test.

### 3.1 Wire the repos into AppState

- [ ] **Step 1: Extend `src/app_state.rs`**

```rust
use std::sync::Arc;

use sqlx::PgPool;

use crate::audit::service::{AuditRecorder, ListAuditEvents};
use crate::tenants::persistence::{OrganisationRepository, RoleRepository};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    pub organisations: Arc<dyn OrganisationRepository>,
    pub roles: Arc<dyn RoleRepository>,
}
```

- [ ] **Step 2: Wire repos in `src/lib.rs::build_app`**

After the audit setup and before building `AppState`:

```rust
use crate::tenants::persistence::{OrganisationRepositoryPg, RoleRepositoryPg};

let organisations: Arc<dyn crate::tenants::persistence::OrganisationRepository> =
    Arc::new(OrganisationRepositoryPg::new(pool.clone()));
let roles: Arc<dyn crate::tenants::persistence::RoleRepository> =
    Arc::new(RoleRepositoryPg::new(pool.clone()));
```

Add `organisations` and `roles` to the `AppState { .. }` literal. Build passes.

- [ ] **Step 3: Extend `MockAppStateBuilder` in `src/testing.rs`**

Add fields and setters:

```rust
// inside struct:
organisations: Option<Arc<dyn crate::tenants::persistence::OrganisationRepository>>,
roles: Option<Arc<dyn crate::tenants::persistence::RoleRepository>>,

// in new():
organisations: None,
roles: None,

// setters:
pub fn with_pg_tenants_repos(mut self) -> Self {
    self.organisations = Some(Arc::new(
        crate::tenants::persistence::OrganisationRepositoryPg::new(self.pool.clone()),
    ));
    self.roles = Some(Arc::new(
        crate::tenants::persistence::RoleRepositoryPg::new(self.pool.clone()),
    ));
    self
}

pub fn organisations(
    mut self,
    r: Arc<dyn crate::tenants::persistence::OrganisationRepository>,
) -> Self {
    self.organisations = Some(r);
    self
}

pub fn roles(mut self, r: Arc<dyn crate::tenants::persistence::RoleRepository>) -> Self {
    self.roles = Some(r);
    self
}

// in build():
AppState {
    pool: self.pool,
    audit_recorder: self.audit_recorder.expect("audit_recorder not set"),
    list_audit_events: self.list_audit_events.expect("list_audit_events not set"),
    organisations: self.organisations.expect("organisations not set"),
    roles: self.roles.expect("roles not set"),
}
```

### 3.2 Seed helpers for tests

- [ ] **Step 4: Create `tests/common/seed.rs`**

```rust
#![allow(dead_code)]

use sqlx::PgPool;
use uuid::Uuid;

/// Insert a user with fixed password hash (tests never log in as this user
/// unless they also bypass auth via minted JWTs).
pub async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'test')",
    )
    .bind(id)
    .bind(username)
    .bind(format!("{username}@test"))
    .execute(pool)
    .await
    .expect("seed user");
    id
}

/// Insert a non-operator organisation and return its id.
pub async fn seed_org(pool: &PgPool, name: &str, business: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO organisations (id, name, business, is_operator) VALUES ($1, $2, $3, FALSE)",
    )
    .bind(id)
    .bind(name)
    .bind(business)
    .execute(pool)
    .await
    .expect("seed org");
    id
}

/// Assign a role to `(user, org)` by role code. Panics if the role does not
/// exist (tests should only use codes from migration 0005: operator_admin,
/// org_owner, org_admin, org_member).
pub async fn grant_role(pool: &PgPool, user: Uuid, org: Uuid, role_code: &str) {
    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
        .bind(role_code)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| panic!("role {role_code} not seeded"));
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(org)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("grant role");
}
```

- [ ] **Step 5: Register the helper module in `tests/common/mod.rs`**

```rust
//! Shared helpers for integration tests. Include via:
//!   #[path = "common/mod.rs"]
//!   mod common;
pub mod auth;
pub mod fixtures;
pub mod seed;
```

- [ ] **Step 6: Verify all existing tests still pass**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Expected: all existing tests still green. The new `seed.rs` is not exercised yet (allowed via `#![allow(dead_code)]`); the `MockAppStateBuilder` additions are also dormant until Task 4.

- [ ] **Step 7: Commit**

```bash
git add src/app_state.rs src/lib.rs src/testing.rs tests/common/
git commit -m "feat(app): wire tenants repos into AppState + test seed helpers"
```

---

## Task 4: Vertical — `POST /api/v1/tenants/organisations` (create)

**Files:**
- Create: `src/tenants/service/create_organisation.rs`
- Modify: `src/tenants/service/mod.rs`
- Create: `src/tenants/interface.rs`
- Modify: `src/tenants/mod.rs`
- Modify: `src/lib.rs` (mount protected router)
- Create: `tests/tenants_service_create_organisation_test.rs`
- Create: `tests/tenants_http_create_organisation_test.rs`

**Intent:** Ship the first endpoint end-to-end: service use case, handler, router mount, service-level tests, and the 4-case HTTP matrix. This is also where `src/tenants/interface.rs` and the tenants router are born.

### 4.1 Service use case

- [ ] **Step 1: Write the failing service test `tests/tenants_service_create_organisation_test.rs`**

```rust
#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use egras::audit::model::AuditEvent;
use egras::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationInput, CreateOrganisationError,
};
use egras::testing::{BlockingAuditRecorder, MockAppStateBuilder, TestPool};
use egras::audit::persistence::{AuditRepository, AuditRepositoryPg};
use uuid::Uuid;

use common::seed::seed_user;

#[tokio::test]
async fn create_organisation_happy_path_returns_summary_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;

    let repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(repo));
    let state = MockAppStateBuilder::new(pool.clone())
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(egras::audit::service::ListAuditEventsImpl::new(
            Arc::new(AuditRepositoryPg::new(pool.clone())),
        )))
        .with_pg_tenants_repos()
        .build();

    let input = CreateOrganisationInput {
        name: "acme".into(),
        business: "retail".into(),
        seed_creator_as_owner: true,
    };
    let out = create_organisation(&state, creator, input).await.unwrap();

    assert_eq!(out.name, "acme");
    assert_eq!(out.business, "retail");
    assert_eq!(out.role_codes, vec!["org_owner"]);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "organisation.created");
}

#[tokio::test]
async fn create_organisation_duplicate_name_is_conflict() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    create_organisation(
        &state,
        creator,
        CreateOrganisationInput {
            name: "acme".into(),
            business: "retail".into(),
            seed_creator_as_owner: false,
        },
    )
    .await
    .unwrap();

    let err = create_organisation(
        &state,
        creator,
        CreateOrganisationInput {
            name: "acme".into(),
            business: "media".into(),
            seed_creator_as_owner: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CreateOrganisationError::DuplicateName));
}
```

Run:

```bash
cargo test --all-features --test tenants_service_create_organisation_test
```

Expected: FAIL (module does not exist).

- [ ] **Step 2: Write `src/tenants/service/create_organisation.rs`**

Contract:

```rust
use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateOrganisationInput {
    pub name: String,
    pub business: String,
    pub seed_creator_as_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOrganisationOutput {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateOrganisationError {
    #[error("organisation name already exists")]
    DuplicateName,
    #[error("invalid name: must be non-empty and ≤ 120 chars")]
    InvalidName,
    #[error("invalid business: must be non-empty and ≤ 120 chars")]
    InvalidBusiness,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn create_organisation(
    state: &AppState,
    creator_user_id: Uuid,
    input: CreateOrganisationInput,
) -> Result<CreateOrganisationOutput, CreateOrganisationError>
```

Behaviour:

1. Validate `name`: 1..=120 chars, trimmed. On failure → `InvalidName`.
2. Validate `business`: 1..=120 chars, trimmed. On failure → `InvalidBusiness`.
3. If `seed_creator_as_owner`: call `organisations.create_with_initial_owner(name, business, creator, "org_owner")`. Else: `organisations.create(name, business)`.
4. Map `RepoError::DuplicateName` → `CreateOrganisationError::DuplicateName`. All other `RepoError` stays wrapped.
5. Emit `AuditEvent::organisation_created(creator, creator_active_org, org.id, &name)` via `state.audit_recorder.record(...)`. Log-and-continue on recorder error (per foundation §7 — never fail the user-visible request for audit-queue failure). Use the creator's active org as `actor_organisation_id`; since the service layer does not receive claims, refactor the signature to pass `actor_org: Uuid` as an extra parameter and adjust tests accordingly.
6. Return `CreateOrganisationOutput { id: org.id, name: org.name, business: org.business, role_codes: if seed_creator_as_owner { vec!["org_owner".into()] } else { vec![] } }`.

- [ ] **Step 3: Re-run the service tests**

```bash
cargo test --all-features --test tenants_service_create_organisation_test
```

Expected: PASS. If tests fail because of the `actor_org` parameter you added in Step 2, update the tests to pass it — seed an operator org via `seed_org` or reuse `creator`'s own future org. Amend the plan's future service tests to match your final signature.

- [ ] **Step 4: Register the module in `src/tenants/service/mod.rs`**

```rust
pub mod create_organisation;
```

### 4.2 HTTP layer

- [ ] **Step 5: Write `src/tenants/interface.rs`**

```rust
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::app_state::AppState;
use crate::auth::jwt::Claims;
use crate::auth::permissions::require_permission;
use crate::errors::AppError;
use crate::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/organisations", post(post_create_organisation))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateOrganisationRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(min = 1, max = 120))]
    pub business: String,
    #[serde(default = "default_true")]
    pub seed_creator_as_owner: bool,
}
fn default_true() -> bool { true }

#[derive(Debug, Serialize, ToSchema)]
pub struct OrganisationBody {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

async fn post_create_organisation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateOrganisationRequest>,
) -> Result<(StatusCode, Json<OrganisationBody>), AppError> {
    // Permission check happens before anything else.
    // (We do not have &Parts here, so we look up PermissionSet via Extensions directly.)
    // Easiest: add a small helper; for now, use the existing require_permission via Extension.
    // If you prefer, introduce a `FromRequestParts`-based `Authorised` extractor.

    // Inline permission lookup:
    // NOTE: this is slightly awkward because axum 0.7 Json extractor consumes the body.
    // Move permission check ahead of the Json extraction by using a `FromRequestParts`
    // extractor that carries PermissionSet. Implementation detail — ensure the
    // precedence holds: 401 (unauth, from AuthLayer) > 403 (missing perm) > 400 (bad body) > 2xx.

    req.validate()
        .map_err(|e| AppError::Validation { errors: validation_errors_to_map(e) })?;

    let out = create_organisation(
        &state,
        claims.sub,
        CreateOrganisationInput {
            name: req.name,
            business: req.business,
            seed_creator_as_owner: req.seed_creator_as_owner,
        },
    )
    .await
    .map_err(map_service_error)?;

    Ok((
        StatusCode::CREATED,
        Json(OrganisationBody {
            id: out.id,
            name: out.name,
            business: out.business,
            role_codes: out.role_codes,
        }),
    ))
}

fn map_service_error(e: CreateOrganisationError) -> AppError {
    match e {
        CreateOrganisationError::DuplicateName => AppError::Conflict {
            reason: "organisation name already exists".into(),
        },
        CreateOrganisationError::InvalidName | CreateOrganisationError::InvalidBusiness => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("name_or_business".into(), vec![e.to_string()]);
            AppError::Validation { errors: errs }
        }
        CreateOrganisationError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        CreateOrganisationError::Internal(err) => AppError::Internal(err),
    }
}

fn validation_errors_to_map(
    e: validator::ValidationErrors,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    for (field, issues) in e.field_errors() {
        out.insert(
            field.to_string(),
            issues
                .iter()
                .map(|v| v.code.to_string())
                .collect::<Vec<_>>(),
        );
    }
    out
}
```

**Permission enforcement:** `tenants.create` is required. Implement the check by reading `PermissionSet` from request extensions before body extraction, either via a custom extractor or by switching the handler signature to use `Parts`-aware extraction. The exact shape is left to the implementer, but the HTTP tests in Step 8 assert the precedence 401 > 403 > 400 and will fail loudly if the order is wrong.

- [ ] **Step 6: Register `interface` in `src/tenants/mod.rs`**

```rust
pub mod interface;
pub mod model;
pub mod persistence;
pub mod service;
```

- [ ] **Step 7: Mount the tenants router in `src/lib.rs::build_app`**

Inside the protected router builder:

```rust
let protected: Router<AppState> = Router::new()
    .nest("/api/v1/tenants", crate::tenants::interface::router())
    .layer(auth_layer);
```

- [ ] **Step 8: Write `tests/tenants_http_create_organisation_test.rs`**

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{MockAppStateBuilder, TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    // default_for_tests already supplies a ≥32-byte jwt_secret and "egras-test" issuer.
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401_problem_json() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .json(&json!({ "name": "acme", "business": "retail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/auth.unauthenticated");
    app.stop().await;
}

#[tokio::test]
async fn missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await; // org_member lacks tenants.create

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "acme", "business": "retail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/permission.denied");
    app.stop().await;
}

#[tokio::test]
async fn happy_path_creates_org_and_returns_201() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await; // org_owner has tenants.create

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool.clone(), cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "beta", "business": "media" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "beta");
    assert_eq!(body["business"], "media");
    assert_eq!(body["role_codes"], json!(["org_owner"]));

    // Audit row landed via BlockingAuditRecorder path — but build_app uses the
    // ChannelAuditRecorder in production. For this test we assert the DB row
    // eventually appears; the worker flushes synchronously on shutdown.
    app.stop().await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE event_type = 'organisation.created'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn duplicate_name_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let seed = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, seed, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, seed);
    let app = TestApp::spawn(pool, cfg).await;

    // First call succeeds
    let _ = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", &token)
        .json(&json!({ "name": "clash", "business": "retail" }))
        .send()
        .await
        .unwrap();

    // Second call → 409
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "clash", "business": "media" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/resource.conflict");
    app.stop().await;
}
```

**Add `AppConfig::default_for_tests()` in Task 4** — this constructor does not exist yet on `feat/foundation`. Add it to `src/config.rs` as:

```rust
impl AppConfig {
    /// Deterministic test config. JWT secret is ≥ 32 bytes to satisfy `validate`.
    pub fn default_for_tests() -> Self {
        Self {
            database_url: std::env::var("TEST_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://egras:egras@127.0.0.1:15432/postgres".into()),
            database_max_connections: 5,
            bind_address: "127.0.0.1:0".into(),
            jwt_secret: "x".repeat(32), // 32 bytes, satisfies validation
            jwt_ttl_secs: 3600,
            jwt_issuer: "egras-test".into(),
            log_level: "info".into(),
            log_format: "json".into(),
            cors_allowed_origins: String::new(),
            password_reset_ttl_secs: 3600,
            operator_org_name: "operator".into(),
            audit_channel_capacity: 128,
            audit_max_retries: 0,
            audit_retry_backoff_ms_initial: 10,
        }
    }
}
```

Gate it behind `#[cfg(any(test, feature = "testing"))]` so production builds don't expose it. Commit alongside Task 4.

- [ ] **Step 9: Run HTTP tests**

```bash
cargo test --all-features --test tenants_http_create_organisation_test
```

Expected: all four green.

- [ ] **Step 10: Full suite + fmt + clippy**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

- [ ] **Step 11: Commit**

```bash
git add src/ tests/tenants_service_create_organisation_test.rs tests/tenants_http_create_organisation_test.rs
git commit -m "feat(tenants): POST /api/v1/tenants/organisations end-to-end"
```

---

## Task 5: Vertical — `GET /api/v1/tenants/me/organisations` (list mine)

**Files:**
- Create: `src/tenants/service/list_my_organisations.rs`
- Modify: `src/tenants/service/mod.rs`
- Modify: `src/tenants/interface.rs`
- Create: `tests/tenants_service_list_my_organisations_test.rs`
- Create: `tests/tenants_http_list_my_organisations_test.rs`

**Intent:** Authenticated caller lists the organisations they are a member of, paginated by a base64url-encoded `(created_at, id)` cursor.

### 5.1 Service

- [ ] **Step 1: Write the failing service test `tests/tenants_service_list_my_organisations_test.rs`**

```rust
#[path = "common/mod.rs"]
mod common;

use egras::testing::{MockAppStateBuilder, TestPool};
use egras::tenants::service::list_my_organisations::{
    list_my_organisations, ListMyOrganisationsInput,
};

use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn list_my_organisations_returns_only_caller_orgs() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let o1 = seed_org(&pool, "alice-1", "retail").await;
    let o2 = seed_org(&pool, "alice-2", "retail").await;
    let _ohidden = seed_org(&pool, "bob-only", "retail").await;

    grant_role(&pool, alice, o1, "org_owner").await;
    grant_role(&pool, alice, o2, "org_member").await;
    grant_role(&pool, bob, _ohidden, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let page = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: None,
            limit: 50,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.items.iter().all(|o| o.name.starts_with("alice-")));
}

#[tokio::test]
async fn list_my_organisations_paginates() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    for i in 0..3 {
        let o = seed_org(&pool, &format!("o-{i}"), "retail").await;
        grant_role(&pool, alice, o, "org_owner").await;
    }
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let page1 = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: None,
            limit: 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.next_cursor.is_some());

    let page2 = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: page1.next_cursor,
            limit: 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(page2.next_cursor.is_none());
}
```

- [ ] **Step 2: Implement `src/tenants/service/list_my_organisations.rs`**

Contract:

```rust
use crate::app_state::AppState;
use crate::tenants::model::OrganisationCursor;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ListMyOrganisationsInput {
    pub after: Option<String>, // base64url-encoded OrganisationCursor
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListMyOrganisationsOutput {
    pub items: Vec<OrganisationSummaryDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OrganisationSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] crate::tenants::persistence::RepoError),
}

pub async fn list_my_organisations(
    state: &AppState,
    caller: Uuid,
    input: ListMyOrganisationsInput,
) -> Result<ListMyOrganisationsOutput, ListError>
```

Behaviour:

1. Clamp `limit` to `1..=100` (default 50 if 0). Return `InvalidCursor` if the user passes `limit > 100` — no, clamp silently.
2. If `input.after` is `Some`: base64url-decode to JSON, parse as `OrganisationCursor`; on failure return `ListError::InvalidCursor`. Otherwise pass `None`.
3. Call `state.organisations.list_for_user(caller, cursor, limit + 1)`.
4. If the repo returned more than `limit` rows, the last row is the next cursor. Encode as base64url(JSON(cursor)). Truncate items to `limit`.
5. Return DTO list + optional cursor string.

Cursor encoding helper lives in a small private function; extract to `crate::tenants::model::encode_cursor` / `decode_cursor` if you prefer shared utility — Task 6 reuses the same shape for `MembershipCursor`.

- [ ] **Step 3: Run the service test**

```bash
cargo test --all-features --test tenants_service_list_my_organisations_test
```

Expected: PASS.

### 5.2 HTTP

- [ ] **Step 4: Add the route in `src/tenants/interface.rs`**

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/organisations", post(post_create_organisation))
        .route("/me/organisations", get(get_list_my_organisations))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PagedOrganisations {
    pub items: Vec<OrganisationBody>,
    pub next_cursor: Option<String>,
}

async fn get_list_my_organisations(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Result<Json<PagedOrganisations>, AppError> {
    let out = list_my_organisations(
        &state,
        claims.sub,
        ListMyOrganisationsInput {
            after: q.after,
            limit: q.limit.unwrap_or(50),
        },
    )
    .await
    .map_err(map_list_error)?;

    Ok(Json(PagedOrganisations {
        items: out
            .items
            .into_iter()
            .map(|o| OrganisationBody {
                id: o.id,
                name: o.name,
                business: o.business,
                role_codes: o.role_codes,
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}

fn map_list_error(e: ListError) -> AppError {
    match e {
        ListError::InvalidCursor => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("after".into(), vec!["invalid_cursor".into()]);
            AppError::Validation { errors: errs }
        }
        ListError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    }
}
```

**No permission required** beyond "authenticated" — but the `AuthLayer` already enforces that. Do NOT call `require_permission` here.

- [ ] **Step 5: Write `tests/tenants_http_list_my_organisations_test.rs`**

Same 4-case matrix, adapted:

- **Unauth:** GET without Authorization → 401 + `auth.unauthenticated`.
- **Missing permission:** skip — this endpoint requires no explicit perm; replace with a **"caller gets only their own orgs"** test that seeds orgs for two users and asserts only the caller's orgs come back.
- **Happy path:** seed 3 orgs for caller, GET with `limit=2`, assert 2 items + non-null `next_cursor`, then GET with the cursor and assert 1 remaining item + null cursor.
- **Domain failure:** GET with `?after=not-a-real-cursor` → 400 + `validation.invalid_request`.

Write each as a separate `#[tokio::test]`. Mirror the structure of Task 4's HTTP test; use `reqwest::Client::new().get(...)` and pass `?after=<cursor>&limit=<n>` as query params.

- [ ] **Step 6: Run + fmt + clippy**

```bash
cargo test --all-features --test tenants_http_list_my_organisations_test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

- [ ] **Step 7: Commit**

```bash
git add src/tenants/ tests/tenants_service_list_my_organisations_test.rs tests/tenants_http_list_my_organisations_test.rs
git commit -m "feat(tenants): GET /api/v1/tenants/me/organisations end-to-end"
```

---

## Task 6: Vertical — `GET /api/v1/tenants/organisations/{id}/members`

**Files:**
- Create: `src/tenants/service/list_organisation_members.rs`
- Modify: `src/tenants/service/mod.rs`
- Modify: `src/tenants/interface.rs`
- Create: `tests/tenants_service_list_organisation_members_test.rs`
- Create: `tests/tenants_http_list_organisation_members_test.rs`

**Intent:** List members of an organisation. Enforces `tenants.members.list` permission AND the cross-org rule (non-members without `tenants.manage_all` get 404, per foundation §3.5).

### 6.1 Service

- [ ] **Step 1: Write the failing service test**

```rust
// tests/tenants_service_list_organisation_members_test.rs
#[path = "common/mod.rs"]
mod common;

use egras::testing::{MockAppStateBuilder, TestPool};
use egras::tenants::service::list_organisation_members::{
    list_organisation_members, ListMembersError, ListMembersInput,
};

use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn list_organisation_members_happy_path() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    grant_role(&pool, bob, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let page = list_organisation_members(
        &state,
        alice,
        /* is_operator = */ false,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 2);
}

#[tokio::test]
async fn non_member_without_manage_all_gets_not_found() {
    let pool = TestPool::fresh().await.pool;
    let mallory = seed_user(&pool, "mallory").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let err = list_organisation_members(
        &state,
        mallory,
        /* is_operator = */ false,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ListMembersError::NotFound));
}

#[tokio::test]
async fn operator_bypass_sees_non_member_org() {
    let pool = TestPool::fresh().await.pool;
    let op = seed_user(&pool, "op").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let page = list_organisation_members(
        &state,
        op,
        /* is_operator = */ true,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
}
```

- [ ] **Step 2: Implement `src/tenants/service/list_organisation_members.rs`**

Contract:

```rust
#[derive(Debug, Clone)]
pub struct ListMembersInput {
    pub organisation_id: Uuid,
    pub after: Option<String>,   // base64url cursor of MembershipCursor
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListMembersOutput {
    pub items: Vec<MemberSummaryDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemberSummaryDto {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListMembersError {
    #[error("not found")]
    NotFound,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] crate::tenants::persistence::RepoError),
}

pub async fn list_organisation_members(
    state: &AppState,
    caller: Uuid,
    is_operator: bool, // true iff caller has `tenants.manage_all`
    input: ListMembersInput,
) -> Result<ListMembersOutput, ListMembersError>
```

Behaviour:

1. Clamp `limit` to `1..=100`.
2. Decode cursor (shared helper from Task 5) → `Option<MembershipCursor>`. On failure → `InvalidCursor`.
3. **Cross-org check:** if `!is_operator`, call `state.organisations.is_member(caller, organisation_id)`. If `false`, return `ListMembersError::NotFound`. (Do NOT leak existence to non-members.)
4. Call `state.organisations.list_members(...)`. Build next_cursor the same way as Task 5 (fetch `limit+1`, truncate).

- [ ] **Step 3: Run the service tests**

```bash
cargo test --all-features --test tenants_service_list_organisation_members_test
```

Expected: PASS.

### 6.2 HTTP

- [ ] **Step 4: Add handler and route**

In `src/tenants/interface.rs`:

```rust
.route("/organisations/:id/members", get(get_list_members))
```

Handler outline:

```rust
async fn get_list_members(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Extension(perms): Extension<crate::auth::permissions::PermissionSet>,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Result<Json<PagedMembers>, AppError> {
    // Permission: require `tenants.members.list` explicitly.
    if !perms.has("tenants.members.list") && !perms.is_operator_over_tenants() {
        return Err(AppError::PermissionDenied {
            code: "tenants.members.list".into(),
        });
    }

    let is_operator = perms.is_operator_over_tenants();
    let out = list_organisation_members(
        &state,
        claims.sub,
        is_operator,
        ListMembersInput {
            organisation_id: org_id,
            after: q.after,
            limit: q.limit.unwrap_or(50),
        },
    )
    .await
    .map_err(|e| match e {
        ListMembersError::NotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        ListMembersError::InvalidCursor => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("after".into(), vec!["invalid_cursor".into()]);
            AppError::Validation { errors: errs }
        }
        ListMembersError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    })?;

    Ok(Json(PagedMembers {
        items: out
            .items
            .into_iter()
            .map(|m| MemberBody {
                user_id: m.user_id,
                username: m.username,
                email: m.email,
                role_codes: m.role_codes,
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}

#[derive(Debug, Serialize)]
pub struct MemberBody {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PagedMembers {
    pub items: Vec<MemberBody>,
    pub next_cursor: Option<String>,
}
```

Note the precedence: permission check (403) comes before the cross-org check (404). This matches foundation §4.

- [ ] **Step 5: Write `tests/tenants_http_list_organisation_members_test.rs`**

4-case matrix:

- **Unauth** (no Authorization) → 401.
- **Missing permission** (member but role lacks `tenants.members.list`): create a custom role-permission setup OR use `org_owner` minus one permission — simpler is to seed a user as a plain member of an empty custom test role. If that requires too much plumbing, defer this to Plan 2b and **document the gap in the commit message**. Acceptable alternative: assert a caller who is NOT a member and is NOT operator gets 404, and separately assert that an operator call without the `tenants.members.list` permission (cannot happen in practice because `operator_admin` has it) is skipped. Pick whichever produces a real assertion; note which.
- **Happy path:** caller is member of org with `org_owner` role; GET returns 200 + members.
- **Domain failure:** caller is not a member and not operator; GET returns 404 + `resource.not_found`.

- [ ] **Step 6: Run + fmt + clippy**

```bash
cargo test --all-features --test tenants_http_list_organisation_members_test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

- [ ] **Step 7: Commit**

```bash
git add src/tenants/ tests/tenants_service_list_organisation_members_test.rs tests/tenants_http_list_organisation_members_test.rs
git commit -m "feat(tenants): GET /api/v1/tenants/organisations/{id}/members end-to-end"
```

---

## Task 7: Vertical — `POST /api/v1/tenants/organisations/{id}/memberships` (assign role)

**Files:**
- Create: `src/tenants/service/assign_role.rs`
- Modify: `src/tenants/service/mod.rs`
- Modify: `src/tenants/interface.rs`
- Create: `tests/tenants_service_assign_role_test.rs`
- Create: `tests/tenants_http_assign_role_test.rs`

**Intent:** Assign a role to a user in an organisation. Enforces `tenants.roles.assign` permission + the cross-org rule. Idempotent: re-assigning an existing `(user, org, role)` triple returns 200 (not 409).

### 7.1 Service

- [ ] **Step 1: Write the failing service test `tests/tenants_service_assign_role_test.rs`**

```rust
#[path = "common/mod.rs"]
mod common;

use egras::testing::{MockAppStateBuilder, TestPool};
use egras::tenants::service::assign_role::{
    assign_role, AssignRoleError, AssignRoleInput, AssignRoleOutput,
};

use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn assign_role_happy_path_adds_role_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let AssignRoleOutput { was_new } = assign_role(
        &state,
        actor,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_admin".into(),
        },
    )
    .await
    .unwrap();
    assert!(was_new);
}

#[tokio::test]
async fn assign_role_idempotent_on_repeat() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_admin").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let out = assign_role(
        &state,
        actor,
        false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_admin".into(),
        },
    )
    .await
    .unwrap();
    // `was_new = false` because the row already existed.
    assert!(!out.was_new);
}

#[tokio::test]
async fn assign_role_unknown_role_code_is_validation() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let err = assign_role(
        &state,
        actor,
        false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "no_such_role".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AssignRoleError::UnknownRoleCode));
}

#[tokio::test]
async fn assign_role_non_member_actor_gets_not_found() {
    let pool = TestPool::fresh().await.pool;
    let outsider = seed_user(&pool, "mallory").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    // outsider is NOT a member of org.

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .build();

    let err = assign_role(
        &state,
        outsider,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_member".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AssignRoleError::NotFound));
}
```

- [ ] **Step 2: Implement `src/tenants/service/assign_role.rs`**

Contract:

```rust
#[derive(Debug, Clone)]
pub struct AssignRoleInput {
    pub organisation_id: Uuid,
    pub target_user_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignRoleOutput {
    pub was_new: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AssignRoleError {
    #[error("not found")]
    NotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error("unknown target user")]
    UnknownUser,
    #[error(transparent)]
    Repo(#[from] crate::tenants::persistence::RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn assign_role(
    state: &AppState,
    actor: Uuid,
    is_operator: bool, // `tenants.manage_all`
    input: AssignRoleInput,
) -> Result<AssignRoleOutput, AssignRoleError>
```

Behaviour:

1. **Cross-org check:** if `!is_operator`, verify `state.organisations.is_member(actor, organisation_id)`. If `false` → `NotFound`.
2. **Resolve role:** `state.roles.find_by_code(&role_code)`. `None` → `UnknownRoleCode`.
3. **Verify target membership exists:** `state.organisations.is_member(target_user_id, organisation_id)`. If `false`, the target is not a member of this org. **Decision: require the target to already be a member before a role can be assigned** (Plan 2b introduces add-user). Return `UnknownUser` in this case (maps to 400 Validation at HTTP layer).
4. **Check pre-existence for idempotency:** query `SELECT EXISTS(SELECT 1 FROM user_organisation_roles WHERE user_id=$1 AND organisation_id=$2 AND role_id=$3)`. This can be done via a new tiny repo method `RoleRepository::has_role(user, org, role_id) -> Result<bool, RepoError>`. Add it to the trait (and impl) as part of this task. Store the result as `was_new = !already`.
5. Call `state.roles.assign(target_user_id, organisation_id, role.id)`. (Idempotent via `ON CONFLICT DO NOTHING` — see Task 2.)
6. If `was_new`, emit `AuditEvent::organisation_role_assigned(actor, actor_active_org, organisation_id, target_user_id, &role_code)` via `state.audit_recorder`. Do NOT emit when the row was already present.
7. Return `AssignRoleOutput { was_new }`.

**Trait extension:** Add to `RoleRepository`:

```rust
async fn has_role(
    &self,
    user_id: Uuid,
    organisation_id: Uuid,
    role_id: Uuid,
) -> Result<bool, RepoError>;
```

And implement it on `RoleRepositoryPg` using a single `EXISTS` query. Update Task 2's tests if the trait change breaks them (it should not, since they don't mock `RoleRepository`).

- [ ] **Step 3: Run the service tests**

```bash
cargo test --all-features --test tenants_service_assign_role_test
```

Expected: PASS. (The 4th test — `non_member_actor_gets_not_found` — depends on the cross-org check firing before role resolution.)

### 7.2 HTTP

- [ ] **Step 4: Add route and handler in `src/tenants/interface.rs`**

```rust
.route("/organisations/:id/memberships", post(post_assign_role))
```

Handler:

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct AssignRoleRequest {
    pub user_id: Uuid,
    #[validate(length(min = 1, max = 64))]
    pub role_code: String,
}

#[derive(Debug, Serialize)]
pub struct AssignRoleResponseBody {
    pub assigned: bool, // true iff this call created the row
}

async fn post_assign_role(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Extension(perms): Extension<crate::auth::permissions::PermissionSet>,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    Json(req): Json<AssignRoleRequest>,
) -> Result<(StatusCode, Json<AssignRoleResponseBody>), AppError> {
    if !perms.has("tenants.roles.assign") && !perms.is_operator_over_tenants() {
        return Err(AppError::PermissionDenied {
            code: "tenants.roles.assign".into(),
        });
    }
    req.validate()
        .map_err(|e| AppError::Validation { errors: validation_errors_to_map(e) })?;

    let out = assign_role(
        &state,
        claims.sub,
        perms.is_operator_over_tenants(),
        AssignRoleInput {
            organisation_id: org_id,
            target_user_id: req.user_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(|e| match e {
        AssignRoleError::NotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        AssignRoleError::UnknownRoleCode => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("role_code".into(), vec!["unknown_role_code".into()]);
            AppError::Validation { errors: errs }
        }
        AssignRoleError::UnknownUser => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("user_id".into(), vec!["not_a_member".into()]);
            AppError::Validation { errors: errs }
        }
        AssignRoleError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        AssignRoleError::Internal(err) => AppError::Internal(err),
    })?;

    Ok((
        StatusCode::OK,
        Json(AssignRoleResponseBody { assigned: out.was_new }),
    ))
}
```

- [ ] **Step 5: Write `tests/tenants_http_assign_role_test.rs`**

4-case matrix:

- **Unauth:** no Authorization → 401 + `auth.unauthenticated`.
- **Missing permission:** seed the caller as `org_member` (which lacks `tenants.roles.assign`) and assert 403 + `permission.denied`.
- **Happy path:** caller is `org_owner`, target is `org_member`; assign `org_admin` → 200, body `{"assigned": true}`. Assert audit row with `event_type = "organisation.role_assigned"` exists in the DB after `app.stop().await`.
- **Domain failure:** pass `role_code: "no_such"` → 400 + `validation.invalid_request` with `errors.role_code`.

Use the same fixture + `TestApp::spawn` pattern from Task 4.

- [ ] **Step 6: Run + fmt + clippy**

```bash
cargo test --all-features --test tenants_http_assign_role_test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

- [ ] **Step 7: Commit**

```bash
git add src/tenants/ tests/tenants_service_assign_role_test.rs tests/tenants_http_assign_role_test.rs
git commit -m "feat(tenants): POST /api/v1/tenants/organisations/{id}/memberships end-to-end"
```

---

## Task 8: End-to-end smoke + OpenAPI registration

**Files:**
- Modify: `src/tenants/interface.rs` (utoipa annotations if not already present)
- Modify: wherever the egras OpenAPI root lives (Plan 1 Task 24 or similar; check `src/lib.rs` or a dedicated `openapi.rs`)

**Intent:** Prove the four endpoints are reachable in `docker compose up`, and advertise them in Swagger-UI.

- [ ] **Step 1: Annotate all four handlers with `#[utoipa::path(...)]`** with tags `tenants`, correct status codes per §2 of the design, and register the DTOs with `ToSchema`. Add the paths to the root `OpenApi` derive.

- [ ] **Step 2: Build and start compose**

```bash
docker compose up --build -d
sleep 3
curl -fsSL http://localhost:8080/health
curl -fsSL http://localhost:8080/ready
curl -fsSL http://localhost:8080/swagger-ui/ | head -c 200
docker compose down
```

Expected: health/ready return 200; the four `/api/v1/tenants/*` paths show up in the Swagger UI OpenAPI JSON.

- [ ] **Step 3: Full local regression**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

All green.

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "docs(openapi): advertise tenants vertical slice endpoints"
```

- [ ] **Step 5: Push the branch + open CI**

```bash
git push -u origin feat/tenants
```

Wait for CI green. If CI is red, fix on this branch — do not merge until CI is green.

---

## Acceptance

Plan 2a is complete when, on `feat/tenants`:

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo test --all-features` passes (service-level + HTTP tests for all four endpoints).
- `docker compose up --build` boots, `/health` and `/ready` return 200, and Swagger UI lists the four `/api/v1/tenants/*` paths.
- CI green on the branch.
- Every write endpoint emits exactly one audit event on success, verified in at least one HTTP test per endpoint.
- Cross-organisation 404 behaviour is asserted for `list_organisation_members` and `assign_role`.

Plan 2b (add-user, remove-user, operator-only security) follows on a separate branch.

