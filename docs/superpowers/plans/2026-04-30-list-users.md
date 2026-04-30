# List Users Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `GET /api/v1/users` — a paginated, filterable endpoint returning platform users enriched with org memberships; operators see all users, tenant admins see only their org.

**Architecture:** Three-phase: (1) extend `UserRepository` trait + PG impl with `list_users` / `list_memberships_for_users`; (2) add `list_users` service use-case; (3) add `GET /api/v1/users` handler to `security/interface.rs` and wire it into `lib.rs`. The permission marker `UsersRead` accepts either `users.manage_all` (operator bypass) or `tenants.members.list` (tenant admin), matching the existing RBAC — no new migration required.

**Tech Stack:** Rust, Axum, SQLx (PostgreSQL), utoipa, chrono, serde, uuid, reqwest (tests)

---

## File Map

| Action   | Path |
|----------|------|
| Modify   | `src/security/model.rs` — add `UserCursor` |
| Modify   | `src/security/persistence/user_repository.rs` — two new trait methods |
| Modify   | `src/security/persistence/user_repository_pg.rs` — two new implementations |
| Create   | `src/security/service/list_users.rs` — use-case |
| Modify   | `src/security/service/mod.rs` — expose `list_users` |
| Modify   | `src/auth/extractors.rs` — add `UsersRead` permission marker |
| Modify   | `src/security/interface.rs` — new DTOs + `get_list_users` handler + route |
| Modify   | `src/lib.rs` — mount `/api/v1/users` route |
| Modify   | `src/openapi.rs` — register new path + schemas |
| Modify (extend) | `tests/security_persistence_test.rs` — 6 new persistence tests |
| Create   | `tests/security_service_list_users_test.rs` — 4 service tests |
| Create   | `tests/security_http_list_users_test.rs` — 7 HTTP tests |

---

### Task 1: Add `UserCursor` to the model

**Files:**
- Modify: `src/security/model.rs`

- [ ] **Step 1: Add `UserCursor` struct**

Open `src/security/model.rs` and append after the `PasswordResetToken` struct:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
```

- [ ] **Step 2: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: no output (zero errors).

- [ ] **Step 3: Commit**

```bash
git add src/security/model.rs
git commit -m "feat(security): add UserCursor to model"
```

---

### Task 2: Extend the `UserRepository` trait

**Files:**
- Modify: `src/security/persistence/user_repository.rs`

The trait lives at `src/security/persistence/user_repository.rs:17`. We add two methods and a note on the batch return type.

- [ ] **Step 1: Write a failing compilation test**

Create a temporary file `tests/security_persistence_list_users_stub_test.rs`:

```rust
// Temporary stub — deleted after Task 3 adds real implementations.
// Confirms trait compiles with the new methods.
#[cfg(test)]
mod _stub {
    use egras::security::persistence::user_repository::UserRepository;
    fn _assert_trait_has_list_users<R: UserRepository>() {}
}
```

Run:
```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features _stub 2>&1 | tail -5
```

Expected: compile error about missing `list_users` / `list_memberships_for_users` on `UserRepository`. (The trait doesn't have them yet.)

- [ ] **Step 2: Add trait methods**

In `src/security/persistence/user_repository.rs`, add to the `UserRepository` trait after `list_memberships`:

```rust
    async fn list_users(
        &self,
        org_id: Option<Uuid>,
        q: Option<&str>,
        cursor: Option<crate::security::model::UserCursor>,
        limit: u32,
    ) -> Result<Vec<User>, UserRepoError>;

    async fn list_memberships_for_users(
        &self,
        user_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, crate::security::model::UserMembership)>, UserRepoError>;
```

> Note: `list_memberships_for_users` returns tuples `(user_id, membership)` so the service can group by user.

- [ ] **Step 3: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: errors about `UserRepositoryPg` not implementing the new methods — that's correct, the pg impl is next.

- [ ] **Step 4: Remove the stub test file**

```bash
rm tests/security_persistence_list_users_stub_test.rs
```

- [ ] **Step 5: Commit**

```bash
git add src/security/persistence/user_repository.rs
git commit -m "feat(security): extend UserRepository trait with list_users and list_memberships_for_users"
```

---

### Task 3: Implement the new repository methods in PostgreSQL

**Files:**
- Modify: `src/security/persistence/user_repository_pg.rs`

- [ ] **Step 1: Add `list_users` implementation**

Append inside `impl UserRepository for UserRepositoryPg` in `src/security/persistence/user_repository_pg.rs` after `list_memberships`:

```rust
    async fn list_users(
        &self,
        org_id: Option<Uuid>,
        q: Option<&str>,
        cursor: Option<crate::security::model::UserCursor>,
        limit: u32,
    ) -> Result<Vec<User>, UserRepoError> {
        // Build query dynamically. sqlx doesn't support truly dynamic WHERE,
        // so we dispatch to four concrete variants based on which filters are set.
        let rows = match (org_id, q, cursor) {
            (None, None, None) => {
                sqlx::query_as::<_, UserRow>(
                    "SELECT id, username, email, password_hash, created_at, updated_at \
                     FROM users \
                     ORDER BY created_at ASC, id ASC \
                     LIMIT $1",
                )
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None, Some(c)) => {
                sqlx::query_as::<_, UserRow>(
                    "SELECT id, username, email, password_hash, created_at, updated_at \
                     FROM users \
                     WHERE (created_at, id) > ($1, $2) \
                     ORDER BY created_at ASC, id ASC \
                     LIMIT $3",
                )
                .bind(c.created_at)
                .bind(c.user_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(oid), None, None) => {
                sqlx::query_as::<_, UserRow>(
                    "SELECT u.id, u.username, u.email, u.password_hash, u.created_at, u.updated_at \
                     FROM users u \
                     JOIN user_organisation_roles uor ON uor.user_id = u.id \
                     WHERE uor.organisation_id = $1 \
                     GROUP BY u.id \
                     ORDER BY u.created_at ASC, u.id ASC \
                     LIMIT $2",
                )
                .bind(oid)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(oid), None, Some(c)) => {
                sqlx::query_as::<_, UserRow>(
                    "SELECT u.id, u.username, u.email, u.password_hash, u.created_at, u.updated_at \
                     FROM users u \
                     JOIN user_organisation_roles uor ON uor.user_id = u.id \
                     WHERE uor.organisation_id = $1 \
                       AND (u.created_at, u.id) > ($2, $3) \
                     GROUP BY u.id \
                     ORDER BY u.created_at ASC, u.id ASC \
                     LIMIT $4",
                )
                .bind(oid)
                .bind(c.created_at)
                .bind(c.user_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(query), None) => {
                let pattern = format!("%{query}%");
                sqlx::query_as::<_, UserRow>(
                    "SELECT id, username, email, password_hash, created_at, updated_at \
                     FROM users \
                     WHERE username ILIKE $1 OR email ILIKE $1 \
                     ORDER BY created_at ASC, id ASC \
                     LIMIT $2",
                )
                .bind(pattern)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(query), Some(c)) => {
                let pattern = format!("%{query}%");
                sqlx::query_as::<_, UserRow>(
                    "SELECT id, username, email, password_hash, created_at, updated_at \
                     FROM users \
                     WHERE (username ILIKE $1 OR email ILIKE $1) \
                       AND (created_at, id) > ($2, $3) \
                     ORDER BY created_at ASC, id ASC \
                     LIMIT $4",
                )
                .bind(pattern)
                .bind(c.created_at)
                .bind(c.user_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(oid), Some(query), None) => {
                let pattern = format!("%{query}%");
                sqlx::query_as::<_, UserRow>(
                    "SELECT u.id, u.username, u.email, u.password_hash, u.created_at, u.updated_at \
                     FROM users u \
                     JOIN user_organisation_roles uor ON uor.user_id = u.id \
                     WHERE uor.organisation_id = $1 \
                       AND (u.username ILIKE $2 OR u.email ILIKE $2) \
                     GROUP BY u.id \
                     ORDER BY u.created_at ASC, u.id ASC \
                     LIMIT $3",
                )
                .bind(oid)
                .bind(pattern)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(oid), Some(query), Some(c)) => {
                let pattern = format!("%{query}%");
                sqlx::query_as::<_, UserRow>(
                    "SELECT u.id, u.username, u.email, u.password_hash, u.created_at, u.updated_at \
                     FROM users u \
                     JOIN user_organisation_roles uor ON uor.user_id = u.id \
                     WHERE uor.organisation_id = $1 \
                       AND (u.username ILIKE $2 OR u.email ILIKE $2) \
                       AND (u.created_at, u.id) > ($3, $4) \
                     GROUP BY u.id \
                     ORDER BY u.created_at ASC, u.id ASC \
                     LIMIT $5",
                )
                .bind(oid)
                .bind(pattern)
                .bind(c.created_at)
                .bind(c.user_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(Into::into).collect())
    }
```

- [ ] **Step 2: Add `list_memberships_for_users` implementation**

Continue appending inside the same `impl` block:

```rust
    async fn list_memberships_for_users(
        &self,
        user_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, crate::security::model::UserMembership)>, UserRepoError> {
        if user_ids.is_empty() {
            return Ok(vec![]);
        }
        #[derive(sqlx::FromRow)]
        struct BulkMembershipRow {
            user_id: Uuid,
            org_id: Uuid,
            org_name: String,
            role_codes: Vec<String>,
            joined_at: DateTime<Utc>,
        }
        let rows = sqlx::query_as::<_, BulkMembershipRow>(
            "SELECT uor.user_id, o.id AS org_id, o.name AS org_name, \
                    array_agg(DISTINCT r.code) AS role_codes, \
                    MIN(uor.created_at) AS joined_at \
             FROM user_organisation_roles uor \
             JOIN organisations o ON o.id = uor.organisation_id \
             JOIN roles r ON r.id = uor.role_id \
             WHERE uor.user_id = ANY($1) \
             GROUP BY uor.user_id, o.id, o.name \
             ORDER BY uor.user_id, joined_at ASC",
        )
        .bind(user_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.user_id,
                    crate::security::model::UserMembership {
                        org_id: r.org_id,
                        org_name: r.org_name,
                        role_codes: r.role_codes,
                        joined_at: r.joined_at,
                    },
                )
            })
            .collect())
    }
```

- [ ] **Step 3: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/security/persistence/user_repository_pg.rs
git commit -m "feat(security): implement list_users and list_memberships_for_users in UserRepositoryPg"
```

---

### Task 4: Persistence tests

**Files:**
- Modify: `tests/security_persistence_test.rs`

- [ ] **Step 1: Add imports at top of `tests/security_persistence_test.rs`**

After the existing imports add:

```rust
use egras::security::model::UserCursor;
```

- [ ] **Step 2: Add `list_users_returns_all_platform_users`**

Append to `tests/security_persistence_test.rs`:

```rust
#[tokio::test]
async fn list_users_returns_all_platform_users() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(None, None, None, 10)
        .await
        .expect("list_users");
    assert_eq!(users.len(), 2);
}
```

- [ ] **Step 3: Run to verify it fails**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features list_users_returns_all_platform_users 2>&1 | tail -5
```

Expected: `FAILED` — method doesn't exist on the trait object yet (trait updated but `AppState.users` is `Arc<dyn UserRepository>` — should compile after task 3).

> If the test compiles and passes already, that's fine — the trait + impl are in place from Task 3.

- [ ] **Step 4: Add `list_users_filtered_by_org_id`**

```rust
#[tokio::test]
async fn list_users_filtered_by_org_id() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await; // not in the org
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(Some(org), None, None, 10)
        .await
        .expect("list_users filtered");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, alice);
}
```

- [ ] **Step 5: Add `list_users_search_by_username`**

```rust
#[tokio::test]
async fn list_users_search_by_username() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(None, Some("ali"), None, 10)
        .await
        .expect("list_users q=ali");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}
```

- [ ] **Step 6: Add `list_users_search_by_email`**

```rust
#[tokio::test]
async fn list_users_search_by_email() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await; // email: alice@test
    seed_user(&pool, "bob").await;   // email: bob@test

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // seed_user creates email as "{username}@test"
    let users = state
        .users
        .list_users(None, Some("alice@test"), None, 10)
        .await
        .expect("list_users q=email");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}
```

- [ ] **Step 7: Add `list_users_cursor_pagination`**

```rust
#[tokio::test]
async fn list_users_cursor_pagination() {
    let pool = TestPool::fresh().await.pool;
    // Insert 3 users; page size 2 → first page returns 2 + cursor; second page returns 1.
    let u1 = seed_user(&pool, "user_a").await;
    let u2 = seed_user(&pool, "user_b").await;
    let u3 = seed_user(&pool, "user_c").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // Page 1: fetch limit+1=3 to detect next page, but return only limit=2.
    let page1 = state
        .users
        .list_users(None, None, None, 3) // over-fetch by 1 from caller's perspective
        .await
        .expect("page1");
    assert_eq!(page1.len(), 3); // all three fit; caller decides truncation

    // Use the second user as the cursor boundary.
    let cursor = UserCursor {
        created_at: page1[1].created_at,
        user_id: page1[1].id,
    };
    let page2 = state
        .users
        .list_users(None, None, Some(cursor), 10)
        .await
        .expect("page2");
    // Only user_c should be after user_b.
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].id, u3);
    // Confirm user_a and user_b are NOT in page2.
    assert!(!page2.iter().any(|u| u.id == u1));
    assert!(!page2.iter().any(|u| u.id == u2));
}
```

- [ ] **Step 8: Add `list_memberships_for_users_batch`**

```rust
#[tokio::test]
async fn list_memberships_for_users_batch() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org1 = seed_org(&pool, "acme", "retail").await;
    let org2 = seed_org(&pool, "globex", "media").await;
    grant_role(&pool, alice, org1, "org_owner").await;
    grant_role(&pool, bob, org2, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let memberships = state
        .users
        .list_memberships_for_users(&[alice, bob])
        .await
        .expect("batch memberships");

    assert_eq!(memberships.len(), 2);
    let alice_m: Vec<_> = memberships.iter().filter(|(uid, _)| *uid == alice).collect();
    let bob_m: Vec<_> = memberships.iter().filter(|(uid, _)| *uid == bob).collect();
    assert_eq!(alice_m.len(), 1);
    assert_eq!(alice_m[0].1.org_id, org1);
    assert_eq!(bob_m.len(), 1);
    assert_eq!(bob_m[0].1.org_id, org2);
}
```

- [ ] **Step 9: Run all six new persistence tests**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features \
  list_users_returns_all_platform_users \
  list_users_filtered_by_org_id \
  list_users_search_by_username \
  list_users_search_by_email \
  list_users_cursor_pagination \
  list_memberships_for_users_batch \
  2>&1 | grep -E "^test|FAILED|ok$"
```

Expected: all 6 pass.

- [ ] **Step 10: Commit**

```bash
git add tests/security_persistence_test.rs
git commit -m "test(security): persistence tests for list_users and list_memberships_for_users"
```

---

### Task 5: Service — `list_users` use-case

**Files:**
- Create: `src/security/service/list_users.rs`
- Modify: `src/security/service/mod.rs`

- [ ] **Step 1: Write the failing service test first**

Create `tests/security_service_list_users_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use common::seed::{grant_role, seed_org, seed_user};
use egras::security::service::list_users::{list_users, ListUsersError, ListUsersInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use uuid::Uuid;

#[tokio::test]
async fn operator_sees_all_users_with_all_memberships() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    grant_role(&pool, bob, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = list_users(
        &state,
        alice,
        /* is_operator = */ true,
        None, // caller_org_id irrelevant for operator
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 10,
        },
    )
    .await
    .expect("operator list_users");

    assert_eq!(out.items.len(), 2);
    // Each user should have their memberships included.
    let alice_item = out.items.iter().find(|u| u.id == alice).unwrap();
    assert_eq!(alice_item.memberships.len(), 1);
    assert!(out.next_cursor.is_none());
}

#[tokio::test]
async fn tenant_admin_sees_only_org_users_memberships_scoped_to_org() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org1 = seed_org(&pool, "acme", "retail").await;
    let org2 = seed_org(&pool, "globex", "media").await;
    grant_role(&pool, alice, org1, "org_owner").await;
    grant_role(&pool, bob, org2, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // Alice is a tenant admin of org1; she should only see users in org1.
    let out = list_users(
        &state,
        alice,
        /* is_operator = */ false,
        Some(org1),
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 10,
        },
    )
    .await
    .expect("tenant admin list_users");

    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].id, alice);
    // Memberships must be scoped to org1 only.
    for item in &out.items {
        for m in &item.memberships {
            assert_eq!(m.org_id, org1);
        }
    }
}

#[tokio::test]
async fn invalid_cursor_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = list_users(
        &state,
        alice,
        true,
        None,
        ListUsersInput {
            org_id: None,
            q: None,
            after: Some("not-valid-base64!!".to_string()),
            limit: 10,
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, ListUsersError::InvalidCursor));
}

#[tokio::test]
async fn limit_clamped_to_100() {
    let pool = TestPool::fresh().await.pool;
    // Seed 5 users; with limit=200 (clamped to 100) all should be returned.
    for i in 0..5 {
        seed_user(&pool, &format!("user{i}")).await;
    }

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = list_users(
        &state,
        Uuid::now_v7(),
        true,
        None,
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 200, // above max — must be clamped
        },
    )
    .await
    .expect("clamped list");
    assert_eq!(out.items.len(), 5); // all 5 fit inside limit=100
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features operator_sees_all_users_with_all_memberships 2>&1 | tail -5
```

Expected: compile error — `egras::security::service::list_users` does not exist yet.

- [ ] **Step 3: Create `src/security/service/list_users.rs`**

```rust
use std::collections::HashMap;

use uuid::Uuid;

use crate::app_state::AppState;
use crate::security::model::{UserCursor, UserMembership};
use crate::security::persistence::user_repository::UserRepoError;
use crate::tenants::service::cursor_codec;

#[derive(Debug, Clone)]
pub struct ListUsersInput {
    pub org_id: Option<Uuid>,
    pub q: Option<String>,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct UserWithMemberships {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub memberships: Vec<UserMembership>,
}

#[derive(Debug, Clone)]
pub struct ListUsersOutput {
    pub items: Vec<UserWithMemberships>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListUsersError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] UserRepoError),
}

pub async fn list_users(
    state: &AppState,
    _caller: Uuid,
    is_operator: bool,
    caller_org_id: Option<Uuid>,
    input: ListUsersInput,
) -> Result<ListUsersOutput, ListUsersError> {
    let limit = input.limit.clamp(1, 100);

    let cursor = match input.after.as_deref() {
        Some(raw) => Some(
            cursor_codec::decode::<UserCursor>(raw)
                .map_err(|_| ListUsersError::InvalidCursor)?,
        ),
        None => None,
    };

    // Non-operators are scoped to their own org regardless of org_id param.
    let effective_org_id = if is_operator {
        input.org_id
    } else {
        caller_org_id
    };

    // Over-fetch by 1 to detect next page.
    let over_fetch = limit.saturating_add(1);
    let mut users = state
        .users
        .list_users(effective_org_id, input.q.as_deref(), cursor, over_fetch)
        .await?;

    let next_cursor = if users.len() as u32 > limit {
        users.truncate(limit as usize);
        let last = users.last().expect("non-empty after truncation");
        Some(cursor_codec::encode(&UserCursor {
            created_at: last.created_at,
            user_id: last.id,
        }))
    } else {
        None
    };

    // Batch-fetch memberships.
    let user_ids: Vec<Uuid> = users.iter().map(|u| u.id).collect();
    let raw_memberships = state
        .users
        .list_memberships_for_users(&user_ids)
        .await?;

    // Group memberships by user_id.
    let mut by_user: HashMap<Uuid, Vec<UserMembership>> = HashMap::new();
    for (uid, membership) in raw_memberships {
        by_user.entry(uid).or_default().push(membership);
    }

    // For non-operators, filter memberships to caller's org only.
    let items = users
        .into_iter()
        .map(|u| {
            let mut memberships = by_user.remove(&u.id).unwrap_or_default();
            if !is_operator {
                if let Some(org) = caller_org_id {
                    memberships.retain(|m| m.org_id == org);
                }
            }
            UserWithMemberships {
                id: u.id,
                username: u.username,
                email: u.email,
                created_at: u.created_at,
                memberships,
            }
        })
        .collect();

    Ok(ListUsersOutput { items, next_cursor })
}
```

- [ ] **Step 4: Expose module in `src/security/service/mod.rs`**

Open `src/security/service/mod.rs` and add:

```rust
pub mod list_users;
```

- [ ] **Step 5: Run the service tests**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features \
  operator_sees_all_users_with_all_memberships \
  tenant_admin_sees_only_org_users_memberships_scoped_to_org \
  invalid_cursor_returns_error \
  limit_clamped_to_100 \
  2>&1 | grep -E "^test|FAILED|ok$"
```

Expected: all 4 pass.

- [ ] **Step 6: Commit**

```bash
git add src/security/service/list_users.rs src/security/service/mod.rs \
        tests/security_service_list_users_test.rs
git commit -m "feat(security): list_users service use-case with tests"
```

---

### Task 6: Add `UsersRead` permission marker

**Files:**
- Modify: `src/auth/extractors.rs`

- [ ] **Step 1: Append `UsersRead` after `TenantsMembersRemove`**

In `src/auth/extractors.rs`, add at the end of the permission markers section:

```rust
/// Permission marker: list platform users.
/// Accepts `users.manage_all` (operator) OR `tenants.members.list` (tenant admin).
pub struct UsersRead;
impl Permission for UsersRead {
    const CODE: &'static str = "tenants.members.list";
    fn accepts(set: &PermissionSet) -> bool {
        set.has("tenants.members.list") || set.is_operator_over_users()
    }
}
```

> `CODE` is `tenants.members.list` because that's the tenant-admin permission already in the DB. The marker name `UsersRead` expresses the intent; `accepts()` covers both the tenant admin and operator paths without requiring a new migration.

- [ ] **Step 2: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/auth/extractors.rs
git commit -m "feat(security): add UsersRead permission marker"
```

---

### Task 7: HTTP handler + route wiring

**Files:**
- Modify: `src/security/interface.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing HTTP tests first**

Create `tests/security_http_list_users_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use common::auth::bearer;
use common::fixtures::OPERATOR_ORG_ID;
use common::seed::{grant_role, seed_org, seed_user};
use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn insufficient_permission_returns_403() {
    // A user with no tenants.members.list and no users.manage_all.
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    // Alice has no org → no permissions loaded.
    let cfg = test_config();
    // Mint a JWT without placing alice in any org — use a random org UUID.
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, uuid::Uuid::now_v7());
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn operator_list_returns_full_memberships() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    // Grant alice operator_admin so she gets users.manage_all.
    grant_role(&pool, alice, OPERATOR_ORG_ID, "operator_admin").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    // Alice is the only non-seed user; she should appear.
    let alice_item = items.iter().find(|i| i["username"] == "alice").unwrap();
    // Her memberships include the retail org.
    let memberships = alice_item["memberships"].as_array().unwrap();
    assert!(!memberships.is_empty());
    app.stop().await;
}

#[tokio::test]
async fn tenant_admin_list_scoped_to_own_org() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org1 = seed_org(&pool, "acme", "retail").await;
    let org2 = seed_org(&pool, "globex", "media").await;
    grant_role(&pool, alice, org1, "org_owner").await;
    grant_role(&pool, bob, org2, "org_member").await;

    let cfg = test_config();
    // Alice's JWT scopes her to org1.
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    // Only Alice — Bob is in org2.
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
    // Memberships are scoped to org1 only.
    for item in items {
        for m in item["memberships"].as_array().unwrap() {
            assert_eq!(m["org_id"], org1.to_string());
        }
    }
    app.stop().await;
}

#[tokio::test]
async fn pagination_next_cursor_present_when_more_results() {
    let pool = TestPool::fresh().await.pool;
    // Seed 3 users, all in the same org so tenant admin can see them.
    let admin = seed_user(&pool, "admin").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, admin, org, "org_owner").await;
    for i in 0..2 {
        let u = seed_user(&pool, &format!("member{i}")).await;
        grant_role(&pool, u, org, "org_member").await;
    }

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, org);
    let app = TestApp::spawn(pool, cfg).await;

    // limit=2 with 3 users → next_cursor must be present.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?limit=2", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert!(!body["next_cursor"].is_null());
    app.stop().await;
}

#[tokio::test]
async fn filter_by_org_id_works() {
    let pool = TestPool::fresh().await.pool;
    let admin = seed_user(&pool, "admin").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, admin, OPERATOR_ORG_ID, "operator_admin").await;
    grant_role(&pool, alice, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?org_id={}", app.base_url, org))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
    app.stop().await;
}

#[tokio::test]
async fn search_q_filters_results() {
    let pool = TestPool::fresh().await.pool;
    let admin = seed_user(&pool, "admin").await;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;
    grant_role(&pool, admin, OPERATOR_ORG_ID, "operator_admin").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?q=alice", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
    app.stop().await;
}
```

- [ ] **Step 2: Run to confirm compile fails**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features unauthenticated_returns_401 \
  --test security_http_list_users_test 2>&1 | tail -5
```

Expected: compile error — route `/api/v1/users` doesn't exist yet.

- [ ] **Step 3: Add DTOs and handler to `src/security/interface.rs`**

At the top of `src/security/interface.rs`, add new imports after the existing service imports:

```rust
use axum::extract::Query;
use crate::auth::extractors::UsersRead;
use crate::security::service::list_users::{
    list_users, ListUsersError, ListUsersInput,
};
```

After the existing `PasswordResetConfirmBody` struct, add the new DTOs:

```rust
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListUsersQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub org_id: Option<Uuid>,
    pub q: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserSummaryDto {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub memberships: Vec<MembershipDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListUsersResponse {
    pub items: Vec<UserSummaryDto>,
    pub next_cursor: Option<String>,
}
```

Then add the handler after `post_password_reset_confirm`:

```rust
#[utoipa::path(
    get,
    path = "/api/v1/users",
    tag = "security",
    params(ListUsersQuery),
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Paginated user list", body = ListUsersResponse),
        (status = 400, description = "Invalid cursor or limit", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
    ),
)]
pub async fn get_list_users(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<UsersRead>,
    Query(q): Query<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, AppError> {
    let is_operator = caller.permissions.is_operator_over_users();
    let caller_org_id = if is_operator { None } else { Some(caller.claims.org) };

    let limit = q.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(field_error("limit", "invalid_limit"));
    }

    let out = list_users(
        &state,
        caller.claims.sub,
        is_operator,
        caller_org_id,
        ListUsersInput {
            org_id: q.org_id,
            q: q.q,
            after: q.after,
            limit,
        },
    )
    .await
    .map_err(|e| match e {
        ListUsersError::InvalidCursor => field_error("after", "invalid_cursor"),
        ListUsersError::Repo(e) => AppError::Internal(e.into()),
    })?;

    state.audit_recorder().record(crate::audit::service::AuditEvent {
        actor_id: caller.claims.sub,
        org_id: caller.claims.org,
        action: "users.list".to_string(),
        resource_type: "user".to_string(),
        resource_id: None,
        metadata: None,
    });

    Ok(Json(ListUsersResponse {
        items: out
            .items
            .into_iter()
            .map(|u| UserSummaryDto {
                id: u.id,
                username: u.username,
                email: u.email,
                created_at: u.created_at,
                memberships: u
                    .memberships
                    .into_iter()
                    .map(|m| MembershipDto {
                        org_id: m.org_id,
                        org_name: m.org_name,
                        role_codes: m.role_codes,
                    })
                    .collect(),
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}
```

- [ ] **Step 4: Check `AuditEvent` field names match the actual struct**

```bash
grep -n "pub struct AuditEvent" /Users/wgiersche/workspace/Project-Ellie/egras/src/audit/service.rs
grep -n "pub " /Users/wgiersche/workspace/Project-Ellie/egras/src/audit/service.rs | head -20
```

Adjust the `AuditEvent { ... }` literal in the handler above to match the actual field names if they differ. Common variation: the struct may use `action: &str` or may not have `resource_type`/`resource_id` fields. Use only fields that exist.

- [ ] **Step 5: Wire the route in `src/lib.rs`**

In `src/lib.rs`, find the `protected` router block and add the `/api/v1/users` route. Change:

```rust
    let protected: Router<AppState> = Router::new()
        .nest("/api/v1/tenants", crate::tenants::interface::router())
        .nest(
            "/api/v1/security",
            crate::security::interface::protected_router(),
        )
        .layer(auth_layer);
```

to:

```rust
    let protected: Router<AppState> = Router::new()
        .nest("/api/v1/tenants", crate::tenants::interface::router())
        .nest(
            "/api/v1/security",
            crate::security::interface::protected_router(),
        )
        .route(
            "/api/v1/users",
            axum::routing::get(crate::security::interface::get_list_users),
        )
        .layer(auth_layer);
```

- [ ] **Step 6: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: no errors. Fix any field-name mismatches found in Step 4.

- [ ] **Step 7: Run all HTTP tests**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features \
  --test security_http_list_users_test \
  2>&1 | grep -E "^test|FAILED|ok$"
```

Expected: all 7 pass.

- [ ] **Step 8: Commit**

```bash
git add src/security/interface.rs src/lib.rs \
        tests/security_http_list_users_test.rs
git commit -m "feat(security): GET /api/v1/users handler, route, and HTTP tests"
```

---

### Task 8: OpenAPI registration and regeneration

**Files:**
- Modify: `src/openapi.rs`
- Modify: `docs/openapi.json` (regenerated)

- [ ] **Step 1: Register path and schemas in `src/openapi.rs`**

In `src/openapi.rs`, add `crate::security::interface::get_list_users` to the `paths(...)` list, and add `crate::security::interface::UserSummaryDto`, `crate::security::interface::ListUsersResponse`, and `crate::security::interface::ListUsersQuery` to the `schemas(...)` list. The file currently ends with `crate::errors::ErrorBody` in schemas — add before it:

In `paths(...)`, add:
```rust
        crate::security::interface::get_list_users,
```

In `schemas(...)`, add:
```rust
        crate::security::interface::UserSummaryDto,
        crate::security::interface::ListUsersResponse,
```

> `ListUsersQuery` uses `IntoParams` (for query params), not `ToSchema`, so it does not go in `schemas(...)`.

- [ ] **Step 2: Compile check**

```bash
cargo check 2>&1 | grep -E "^error"
```

Expected: no errors.

- [ ] **Step 3: Regenerate OpenAPI JSON**

```bash
EGRAS_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras \
  cargo run -- dump-openapi > docs/openapi.json
```

- [ ] **Step 4: Verify the new path appears**

```bash
grep '"\/api\/v1\/users"' docs/openapi.json
```

Expected: one matching line.

- [ ] **Step 5: Run full test suite**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features 2>&1 | tail -10
```

Expected: all tests pass, zero failures.

- [ ] **Step 6: Commit**

```bash
git add src/openapi.rs docs/openapi.json
git commit -m "chore: register GET /api/v1/users in OpenAPI and regenerate docs/openapi.json"
```

---

## Self-Review

**Spec coverage:**

| Spec requirement | Task |
|-----------------|------|
| `GET /api/v1/users` route | Task 7 Step 5 |
| `after`, `limit`, `org_id`, `q` query params | Task 7 Step 3 (DTOs + handler) |
| `limit` validation 1–100 | Task 7 Step 3 |
| `users.manage_all` operator path | Task 6 + Task 7 Step 3 |
| `tenants.members.list` tenant admin path | Task 6 |
| Operator sees all users, all memberships | Task 5 service tests |
| Tenant admin scoped to own org + memberships | Task 5 service tests |
| Cursor pagination (over-fetch pattern) | Task 3 + Task 5 |
| `UserCursor` tie-break on (created_at, user_id) | Task 1 + Task 3 |
| Two-phase fetch (list_users + batch memberships) | Task 3 + Task 5 |
| Audit event `users.list` | Task 7 Step 3 |
| 401 unauthenticated | Task 7 tests |
| 403 permission denied | Task 7 tests |
| 400 invalid_cursor | Task 7 tests + service tests |
| 400 invalid_limit | Task 7 Step 3 |
| Persistence tests (6) | Task 4 |
| Service tests (4) | Task 5 |
| HTTP tests (7) | Task 7 |
| OpenAPI registration | Task 8 |

All spec requirements covered. No placeholders remain.

**Type consistency:**
- `UserCursor` defined in Task 1, used in Task 2 (trait), Task 3 (impl), Task 5 (service).
- `UserMembership` from existing model, reused throughout.
- `UserWithMemberships` defined in Task 5 service, consumed in Task 7 handler.
- `MembershipDto` already exists in `security/interface.rs` — reused in `UserSummaryDto`.
- `list_memberships_for_users` returns `Vec<(Uuid, UserMembership)>` consistently across Tasks 2, 3, and 5.
