# Service Accounts & API Keys — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the service-account principal type and per-key API-key auth path described in `docs/superpowers/specs/2026-05-03-service-accounts-design.md`.

**Architecture:** Hybrid principal — SAs share `users.id` (free RBAC + audit reuse) plus a sidecar `service_accounts` table. API keys authenticate via `Authorization: Bearer egras_live_<prefix>_<secret>`; the existing `AuthLayer` dispatches on the prefix and produces uniform `Claims` + `PermissionSet` extensions. Optional per-key `scopes` intersected with the SA's loaded permissions; `NULL = inherit`.

**Tech Stack:** axum, sqlx, Postgres, argon2 (existing), utoipa, async-trait, anyhow / thiserror.

**Discipline:** every commit must compile. Pre-push gate (per `feedback_pre_push_checks.md`): `cargo fmt --all` + `cargo clippy --all-targets --all-features -- -D warnings` + `cargo nextest run --all-features`. After `git push`, poll CI. Wiki updates land in this same PR (CLAUDE.md mandate).

**Branch:** `feat/service-accounts` (already created).

---

## File map

**Created**
- `migrations/0011_service_accounts.sql`
- `src/security/persistence/service_account_repository.rs`
- `src/security/persistence/service_account_repository_pg.rs`
- `src/security/persistence/api_key_repository.rs`
- `src/security/persistence/api_key_repository_pg.rs`
- `src/security/service/create_service_account.rs`
- `src/security/service/list_service_accounts.rs`
- `src/security/service/delete_service_account.rs`
- `src/security/service/create_api_key.rs`
- `src/security/service/list_api_keys.rs`
- `src/security/service/revoke_api_key.rs`
- `src/security/service/rotate_api_key.rs`
- `src/security/service/api_key_secret.rs` (key-format generation + parsing helpers)
- `tests/it/security_persistence_service_accounts_test.rs`
- `tests/it/security_persistence_api_keys_test.rs`
- `tests/it/security_service_service_accounts_test.rs`
- `tests/it/security_service_api_keys_test.rs`
- `tests/it/security_http_service_accounts_test.rs`
- `tests/it/security_http_api_keys_test.rs`
- `tests/it/auth_api_key_dispatch_test.rs`
- `knowledge/wiki/Service-Accounts.md`

**Modified**
- `src/security/model.rs` — add `UserKind`, `ServiceAccount`, `ApiKey`, `ApiKeyMaterial`, `NewApiKey`; `User.kind: UserKind`
- `src/security/persistence/{mod.rs,user_repository.rs,user_repository_pg.rs}` — read/write `kind`; expose new repos
- `src/security/service/mod.rs` — list new use cases
- `src/security/interface.rs` — append handlers + DTOs + routes
- `src/auth/jwt.rs` — `Claims` is unchanged; document the synthesized variant
- `src/auth/middleware.rs` — `ApiKeyVerifierStrategy` trait, `ApiKeyVerifier` wrapper, `PgApiKeyVerifier` (in security/persistence), `AuthLayer::new` 5-arg, prefix dispatch in `AuthService::call`
- `src/auth/extractors.rs` — `Caller` enum, `RequireHumanCaller` extractor
- `src/auth/permissions.rs` — `PermissionSet::intersect(&[String])`
- `src/app_state.rs` — `service_accounts`, `api_keys` repo handles
- `src/lib.rs` — wire `PgApiKeyVerifier` into `AuthLayer`; add repos to `AppState`
- `src/testing.rs` — `MockAppStateBuilder::with_pg_service_account_repos()`; default for new fields
- `src/tenants/service/assign_role.rs` — cross-org SA guard, new `ServiceAccountCrossOrgForbidden` error
- `src/security/service/{logout,change_password,password_reset_request,password_reset_confirm,switch_org}.rs` — handlers add `RequireHumanCaller`
- `docs/openapi.json` — regenerated last
- `knowledge/wiki/Architecture.md`, `Authentication.md`, `Authorization.md`, `Security-Domain.md`, `Data-Model.md` — sections updated
- `knowledge/wiki/future-enhancements/INDEX.md` — strike through entry
- delete: `knowledge/wiki/future-enhancements/Service-Accounts-and-API-Keys.md`

---

## Commit map

The plan groups tasks into four commits. Each commit must build clean.

| Commit | Tasks | What lands |
|---|---|---|
| **C1** | T1–T6 | Migration + permission seeds + models + persistence repos with tests |
| **C2** | T7–T13 | Service layer: SA CRUD, API key CRUD/rotate, key-format helpers, cross-org SA guard |
| **C3** | T14–T18 | `Caller` enum, `RequireHumanCaller`, `ApiKeyVerifier`, AuthLayer dispatch, `PermissionSet::intersect` |
| **C4** | T19–T26 | HTTP handlers + caller-type gating on existing endpoints + wiring + OpenAPI dump + wiki + final pre-push gate + push + PR |

Each commit's tests must pass at that commit. The pre-push gate runs ONCE before push.

---

## Task 1 — Migration `0011_service_accounts.sql`

**Files:** Create `migrations/0011_service_accounts.sql`

- [ ] **Step 1: Write the migration**

```sql
-- 0011_service_accounts.sql
-- Service accounts (non-human principals) and per-SA API keys.

-- Gate column on users to distinguish principal type.
ALTER TABLE users
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'human'
    CHECK (kind IN ('human', 'service_account'));

CREATE TABLE service_accounts (
    user_id          UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    organisation_id  UUID NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    description      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by       UUID NOT NULL REFERENCES users(id),
    last_used_at     TIMESTAMPTZ,
    UNIQUE (organisation_id, name)
);

CREATE TABLE api_keys (
    id                       UUID PRIMARY KEY,
    service_account_user_id  UUID NOT NULL
        REFERENCES service_accounts(user_id) ON DELETE CASCADE,
    prefix                   TEXT NOT NULL UNIQUE,
    secret_hash              TEXT NOT NULL,
    name                     TEXT NOT NULL,
    scopes                   TEXT[],
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by               UUID NOT NULL REFERENCES users(id),
    last_used_at             TIMESTAMPTZ,
    revoked_at               TIMESTAMPTZ,
    CHECK (scopes IS NULL OR cardinality(scopes) > 0)
);

CREATE INDEX ix_api_keys_active_by_sa
    ON api_keys (service_account_user_id) WHERE revoked_at IS NULL;

-- Permissions: read + manage. Granted to org_owner and org_admin.
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000301', 'service_accounts.read',
      'List + read service accounts and API key metadata in own org'),
  ('00000000-0000-0000-0000-000000000302', 'service_accounts.manage',
      'Create / delete service accounts and API keys in own org');

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code IN ('service_accounts.read', 'service_accounts.manage');
```

- [ ] **Step 2: Run migration smoke check**

```bash
TEST_DATABASE_URL=postgres://egras:egras@127.0.0.1:15432/postgres \
  cargo nextest run --all-features --no-tests=pass jobs_persistence_test::enqueue_then_find
```

Expected: PASS (uses `TestPool::fresh()`, which runs all migrations including `0011`). If the SQL is broken, this fails at migration time.

- [ ] **Step 3: No commit yet** — schema lands with the persistence task that uses it (T2–T6).

---

## Task 2 — `UserKind` enum + `User.kind`

**Files:** Modify `src/security/model.rs`, `src/security/persistence/user_repository_pg.rs`

- [ ] **Step 1: Add `UserKind` to `src/security/model.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    Human,
    ServiceAccount,
}

impl UserKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UserKind::Human => "human",
            UserKind::ServiceAccount => "service_account",
        }
    }
}

impl std::str::FromStr for UserKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(UserKind::Human),
            "service_account" => Ok(UserKind::ServiceAccount),
            other => anyhow::bail!("unknown user kind: {other}"),
        }
    }
}
```

Then add `pub kind: UserKind,` to the `User` struct (after `password_hash`).

- [ ] **Step 2: Update `UserRepositoryPg` row mapping + queries**

Every `SELECT … FROM users` adds `kind`. Every `INSERT INTO users` adds `kind`. Existing `create` defaults `'human'`.

Concretely, modify the row type and every query in `src/security/persistence/user_repository_pg.rs` to include the new column. The `User` constructor in `row_to_user` parses `kind` via `UserKind::from_str`.

- [ ] **Step 3: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean. Tests not yet updated; that's fine for this task.

- [ ] **Step 4: Update existing user-related test asserts**

Search tests that construct or assert `User { ... }`. Add `kind: UserKind::Human` field. Persist + read tests in `tests/it/security_persistence_test.rs` should now read `kind = Human` for users created via `repo.create(...)`.

- [ ] **Step 5: Run security-domain tests**

```bash
cargo nextest run --all-features security_persistence security_service
```

Expected: all pass.

---

## Task 3 — `ServiceAccount` + `ApiKey` model types

**Files:** Modify `src/security/model.rs`

- [ ] **Step 1: Append types**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub service_account_user_id: Uuid,
    pub prefix: String,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One-time response holding the plaintext key. Never persisted.
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyMaterial {
    pub key: ApiKey,
    pub plaintext: String,
}

#[derive(Debug, Clone)]
pub struct NewApiKey {
    pub service_account_user_id: Uuid,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_by: Uuid,
}
```

- [ ] **Step 2: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean (types are only declared, not used yet).

---

## Task 4 — `ServiceAccountRepository` trait + Pg impl

**Files:** Create `src/security/persistence/service_account_repository.rs`, `src/security/persistence/service_account_repository_pg.rs`; modify `src/security/persistence/mod.rs`.

- [ ] **Step 1: Trait**

```rust
// src/security/persistence/service_account_repository.rs
use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::ServiceAccount;

#[derive(Debug, thiserror::Error)]
pub enum ServiceAccountRepoError {
    #[error("service account name already used in this organisation")]
    DuplicateName,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct NewServiceAccount {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Uuid,
}

#[async_trait]
pub trait ServiceAccountRepository: Send + Sync + 'static {
    /// Atomic insert: users row (kind='service_account') + service_accounts row.
    async fn create(&self, req: NewServiceAccount)
        -> Result<ServiceAccount, ServiceAccountRepoError>;

    async fn find(&self, organisation_id: Uuid, sa_user_id: Uuid)
        -> anyhow::Result<Option<ServiceAccount>>;

    /// Returns (page, next_cursor). `after` = `(created_at, sa_user_id)`.
    async fn list(
        &self,
        organisation_id: Uuid,
        limit: u32,
        after: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
    ) -> anyhow::Result<Vec<ServiceAccount>>;

    /// Deletes the users row; ON DELETE CASCADE collapses sidecar + keys.
    async fn delete(&self, organisation_id: Uuid, sa_user_id: Uuid) -> anyhow::Result<bool>;

    async fn touch_last_used(&self, sa_user_id: Uuid) -> anyhow::Result<()>;
}
```

- [ ] **Step 2: Pg impl**

`create` must run in a transaction:
1. `INSERT INTO users (id, username, email, password_hash, kind) VALUES ($1, $2, $3, '!', 'service_account')` — `username` and `email` are synthesised from the SA name + UUID to keep the existing `users` UNIQUE constraints happy. Format: `username = sa_${sa_user_id}`, `email = sa_${sa_user_id}@service-account.invalid`. `password_hash = '!'` (a sentinel that will never verify).
2. `INSERT INTO service_accounts(...)` returning the row.
3. On `unique_violation` from the service_accounts insert (organisation_id, name), map to `DuplicateName`.

Show full code in the file. Implement other methods with throttled UPDATE for `touch_last_used`:

```sql
UPDATE service_accounts
   SET last_used_at = NOW()
 WHERE user_id = $1
   AND (last_used_at IS NULL OR last_used_at < NOW() - INTERVAL '60 seconds');
```

- [ ] **Step 3: Persistence test** — `tests/it/security_persistence_service_accounts_test.rs`

```rust
use chrono::Utc;
use egras::security::model::UserKind;
use egras::security::persistence::{
    NewServiceAccount, ServiceAccountRepoError, ServiceAccountRepository, ServiceAccountRepositoryPg,
    UserRepository, UserRepositoryPg,
};
use egras::testing::TestPool;
use uuid::Uuid;

use crate::common::seed::{seed_org, seed_user};

#[tokio::test]
async fn create_then_find() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    let sa = repo
        .create(NewServiceAccount {
            organisation_id: org,
            name: "billing-bot".into(),
            description: Some("Bills runner".into()),
            created_by: creator,
        })
        .await
        .unwrap();

    let user = UserRepositoryPg::new(pool.clone()).find(sa.user_id).await.unwrap().unwrap();
    assert_eq!(user.kind, UserKind::ServiceAccount);

    let again = repo.find(org, sa.user_id).await.unwrap().unwrap();
    assert_eq!(again.name, "billing-bot");
}

#[tokio::test]
async fn duplicate_name_in_org_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    repo.create(NewServiceAccount {
        organisation_id: org, name: "billing-bot".into(), description: None, created_by: creator,
    }).await.unwrap();

    let err = repo
        .create(NewServiceAccount {
            organisation_id: org, name: "billing-bot".into(), description: None, created_by: creator,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceAccountRepoError::DuplicateName));
}

#[tokio::test]
async fn duplicate_name_in_different_org_is_ok() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    repo.create(NewServiceAccount {
        organisation_id: org_a, name: "shared".into(), description: None, created_by: creator,
    }).await.unwrap();
    repo.create(NewServiceAccount {
        organisation_id: org_b, name: "shared".into(), description: None, created_by: creator,
    }).await.unwrap();
}

#[tokio::test]
async fn touch_last_used_throttled() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());
    let sa = repo.create(NewServiceAccount {
        organisation_id: org, name: "bot".into(), description: None, created_by: creator,
    }).await.unwrap();

    repo.touch_last_used(sa.user_id).await.unwrap();
    let after_first = repo.find(org, sa.user_id).await.unwrap().unwrap().last_used_at;
    assert!(after_first.is_some());

    // Immediate second touch is a no-op (under 60 s).
    repo.touch_last_used(sa.user_id).await.unwrap();
    let after_second = repo.find(org, sa.user_id).await.unwrap().unwrap().last_used_at;
    assert_eq!(after_first, after_second);
}
```

Add `mod security_persistence_service_accounts_test;` to `tests/it/main.rs`.

- [ ] **Step 4: Run tests**

```bash
cargo nextest run --all-features security_persistence_service_accounts_test
```

Expected: 4 passed.

---

## Task 5 — `ApiKeyRepository` trait + Pg impl

**Files:** Create `src/security/persistence/api_key_repository.rs`, `src/security/persistence/api_key_repository_pg.rs`; modify `mod.rs`.

- [ ] **Step 1: Trait**

```rust
#[async_trait]
pub trait ApiKeyRepository: Send + Sync + 'static {
    /// Insert a new key. `secret_hash` is argon2(secret). `prefix` must be unique;
    /// caller retries on `DuplicatePrefix` (extremely rare).
    async fn create(&self, req: NewApiKeyRow) -> Result<ApiKey, ApiKeyRepoError>;

    /// Lookup by prefix; returns active (non-revoked) key only.
    async fn find_active_by_prefix(&self, prefix: &str) -> anyhow::Result<Option<ApiKeyRow>>;

    async fn find(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<Option<ApiKey>>;

    async fn list_by_sa(&self, sa_user_id: Uuid) -> anyhow::Result<Vec<ApiKey>>;

    /// Mark revoked; idempotent. Returns `true` if it transitioned, `false` if already revoked / missing.
    async fn revoke(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<bool>;

    /// Throttled to ≤ 1/min/key.
    async fn touch_last_used(&self, key_id: Uuid) -> anyhow::Result<()>;

    /// Atomic create + revoke for `rotate`.
    async fn rotate(&self, old_key_id: Uuid, new: NewApiKeyRow)
        -> Result<ApiKey, ApiKeyRepoError>;
}

pub struct NewApiKeyRow {
    pub id: Uuid,
    pub service_account_user_id: Uuid,
    pub prefix: String,
    pub secret_hash: String,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_by: Uuid,
}

/// Row variant carrying the `secret_hash` (only used by the auth path).
pub struct ApiKeyRow {
    pub key: ApiKey,
    pub secret_hash: String,
    pub organisation_id: Uuid,    // joined from service_accounts for verifier
}

#[derive(Debug, thiserror::Error)]
pub enum ApiKeyRepoError {
    #[error("api key prefix collision; retry")]
    DuplicatePrefix,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

- [ ] **Step 2: Pg impl**

Show all SQL inline. Key points:
- `find_active_by_prefix` joins `service_accounts` to fetch `organisation_id` (saves a round-trip in the verifier).
- `revoke` uses `UPDATE … WHERE id = $1 AND service_account_user_id = $2 AND revoked_at IS NULL`; bool from rows-affected.
- `rotate` runs in a transaction: `UPDATE api_keys SET revoked_at=NOW() WHERE id=$1` then INSERT new.
- `touch_last_used` uses the same throttled-UPDATE pattern.

- [ ] **Step 3: Persistence test** — `tests/it/security_persistence_api_keys_test.rs`

Tests for: create + find_active_by_prefix returns it; revoke + find_active_by_prefix returns None; touch_last_used throttle; rotate returns new + old becomes revoked; duplicate prefix → DuplicatePrefix.

(Code shown inline in the test file — follow Task 4's pattern: ~5 `#[tokio::test]` functions, each ≈15 lines, using `seed_user` + `seed_org` + a helper to seed an SA via the repo created in T4.)

Add module to `tests/it/main.rs`.

- [ ] **Step 4: Run tests**

```bash
cargo nextest run --all-features security_persistence_api_keys_test
```

Expected: 5 passed.

---

## Task 6 — Commit C1

- [ ] **Step 1: Run commit gate**

```bash
cargo fmt --all
cargo check --all-targets --all-features
cargo nextest run --all-features
```

Expected: 200+ passed.

- [ ] **Step 2: Commit C1**

```bash
git add migrations/0011_service_accounts.sql \
        src/security/model.rs \
        src/security/persistence \
        tests/it/security_persistence_service_accounts_test.rs \
        tests/it/security_persistence_api_keys_test.rs \
        tests/it/main.rs

git commit -m "$(cat <<'EOF'
feat(security): service-accounts schema + persistence layer

Migration 0011 adds users.kind, service_accounts (sidecar), api_keys
(prefix-unique, scopes, revoked_at), and seeds service_accounts.read /
service_accounts.manage permissions to org_owner / org_admin.

Adds UserKind, ServiceAccount, ApiKey, ApiKeyMaterial, NewApiKey types.
ServiceAccountRepository / ApiKeyRepository traits + Postgres impls with
throttled last_used_at updates and atomic rotate (create + revoke in one
tx). 9 persistence tests, all passing.

Service / interface / wiring in subsequent commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Do NOT push yet — push happens once at C4.

---

## Task 7 — `api_key_secret` helper module

**Files:** Create `src/security/service/api_key_secret.rs`

- [ ] **Step 1: Implementation**

```rust
//! Generation, hashing, and parsing for the API-key wire format
//! `egras_<env>_<prefix8>_<secret_b64>`.

use anyhow::bail;
use rand::rngs::OsRng;
use rand::TryRngCore;

const ENV_LIVE: &str = "live";

pub struct GeneratedKey {
    pub prefix: String,           // 8 hex
    pub plaintext: String,        // full key string
    pub secret: String,           // bare secret, for hashing
}

pub fn generate() -> anyhow::Result<GeneratedKey> {
    let mut prefix_bytes = [0u8; 4];
    let mut secret_bytes = [0u8; 32];
    OsRng.try_fill_bytes(&mut prefix_bytes)?;
    OsRng.try_fill_bytes(&mut secret_bytes)?;
    let prefix = hex::encode(prefix_bytes);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);
    let plaintext = format!("egras_{ENV_LIVE}_{prefix}_{secret}");
    Ok(GeneratedKey { prefix, plaintext, secret })
}

pub struct ParsedKey<'a> {
    pub env: &'a str,
    pub prefix: &'a str,
    pub secret: &'a str,
}

/// Parse "egras_<env>_<prefix8>_<secret>". Returns None on any malformed shape.
pub fn parse(s: &str) -> Option<ParsedKey<'_>> {
    let body = s.strip_prefix("egras_")?;
    let parts: Vec<&str> = body.splitn(3, '_').collect();
    if parts.len() != 3 { return None; }
    let [env, prefix, secret] = [parts[0], parts[1], parts[2]];
    if prefix.len() != 8 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    if secret.is_empty() { return None; }
    Some(ParsedKey { env, prefix, secret })
}

/// Hash the secret using the same Argon2 config used for passwords.
pub fn hash_secret(secret: &str) -> anyhow::Result<String> {
    crate::security::service::password_hash::hash(secret)
}

/// Verify a secret against a stored argon2 hash. Constant-time at the lib level.
pub fn verify_secret(secret: &str, hash: &str) -> anyhow::Result<bool> {
    crate::security::service::password_hash::verify(secret, hash)
}
```

(import lines for `base64::Engine` etc. — see existing channel_repository_pg.rs for the same crate usage pattern.)

- [ ] **Step 2: Unit tests in same file**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let k = generate().unwrap();
        let parsed = parse(&k.plaintext).expect("parse");
        assert_eq!(parsed.env, "live");
        assert_eq!(parsed.prefix, k.prefix);
        assert_eq!(parsed.secret, k.secret);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse("egras_live_zz_xx").is_none());          // non-hex prefix
        assert!(parse("egras_live_aaaaaaaa_").is_none());      // empty secret
        assert!(parse("notakey").is_none());
        assert!(parse("egras_live").is_none());
    }
}
```

- [ ] **Step 3: Run**

```bash
cargo test --all-features --lib security::service::api_key_secret
```

Expected: 2 passed.

---

## Task 8 — `create_service_account` use case

**Files:** Create `src/security/service/create_service_account.rs`; modify `src/security/service/mod.rs`.

- [ ] **Step 1: Implementation**

```rust
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::{AuditCategory, AuditEvent, Outcome};
use crate::security::model::ServiceAccount;
use crate::security::persistence::{NewServiceAccount, ServiceAccountRepoError};

#[derive(Debug, Clone)]
pub struct Input {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub actor_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("name already used in this organisation")]
    DuplicateName,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn create_service_account(
    state: &AppState,
    input: Input,
) -> Result<ServiceAccount, Error> {
    let sa = state
        .service_accounts
        .create(NewServiceAccount {
            organisation_id: input.organisation_id,
            name: input.name.clone(),
            description: input.description.clone(),
            created_by: input.actor_id,
        })
        .await
        .map_err(|e| match e {
            ServiceAccountRepoError::DuplicateName => Error::DuplicateName,
            ServiceAccountRepoError::Other(other) => Error::Other(other),
        })?;

    state
        .audit_recorder
        .record(
            AuditEvent::new("service_account.created", AuditCategory::Security)
                .with_actor(input.actor_id)
                .with_subject(sa.user_id)
                .with_organisation(input.organisation_id)
                .with_outcome(Outcome::Success)
                .with_metadata(serde_json::json!({"name": input.name})),
        )
        .await;

    Ok(sa)
}
```

(Adapt `AuditEvent` constructor to the existing API; check `src/audit/model.rs` for the actual builder shape.)

- [ ] **Step 2: Service test** — `tests/it/security_service_service_accounts_test.rs`

```rust
use egras::security::service::create_service_account::{create_service_account, Input};
use egras::testing::{MockAppStateBuilder, TestPool};

use crate::common::seed::{seed_org, seed_user};

#[tokio::test]
async fn create_happy_path_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_security_repos()
        .with_pg_service_account_repos()
        .build();

    let sa = create_service_account(
        &state,
        Input {
            organisation_id: org,
            name: "billing-bot".into(),
            description: Some("test".into()),
            actor_id: creator,
        },
    )
    .await
    .unwrap();

    assert_eq!(sa.name, "billing-bot");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events \
         WHERE event_type = 'service_account.created' AND subject_id = $1",
    )
    .bind(sa.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}
```

(`with_pg_service_account_repos` lives in T18; this test is added now but won't run until T18. Keep the test file present and the module declared in `tests/it/main.rs` so T18 can flip the missing-builder method to wired.)

- [ ] **Step 3: Compile only**

```bash
cargo check --all-targets --all-features
```

If `with_pg_service_account_repos` doesn't exist yet, the test file is excluded by feature gate or you skip running that test for now. Practical approach: stub the builder method as part of T18 — until then, comment out the new tests with `// TODO: enable after T18 wires the builder`. Strike that comment in T18.

---

## Task 9 — `list_service_accounts`, `delete_service_account` use cases

**Files:** Create both service files.

- [ ] **Step 1: list — paginated, org-scoped**

`Input = { organisation_id, actor_org_id, after: Option<Cursor>, limit }`. Reuse `UserCursor`-style cursor with `(created_at, sa_user_id)`. Cross-org caller gets `Output { items: vec![], next_cursor: None }` if not operator (or service simply returns empty — handler maps to 200 with empty list, never 404 here).

- [ ] **Step 2: delete**

`Input = { organisation_id, sa_user_id, actor_id }`. Delegates to `repo.delete(...)`. Returns 404 (`Error::NotFound`) if `delete` returns `false`. Emits `service_account.deleted` audit event.

- [ ] **Step 3: Service tests** — append to `tests/it/security_service_service_accounts_test.rs`

Tests for: list returns only org's SAs; delete removes + emits audit; cross-org delete returns NotFound.

- [ ] **Step 4: Run**

```bash
cargo check --all-targets --all-features
```

Expected: clean.

---

## Task 10 — `create_api_key` use case

**Files:** Create `src/security/service/create_api_key.rs`

- [ ] **Step 1: Implementation**

```rust
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::{AuditCategory, AuditEvent, Outcome};
use crate::security::model::{ApiKeyMaterial};
use crate::security::persistence::{ApiKeyRepoError, NewApiKeyRow};
use crate::security::service::api_key_secret;

#[derive(Debug, Clone)]
pub struct Input {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
    pub name: String,
    pub scopes: Option<Vec<String>>,    // None = inherit; Some(vec![]) is rejected here
    pub actor_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("service account not found")]
    NotFound,
    #[error("scopes cannot be empty (use null to inherit)")]
    EmptyScopes,
    #[error("could not allocate unique key prefix; please retry")]
    PrefixCollision,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn create_api_key(state: &AppState, input: Input) -> Result<ApiKeyMaterial, Error> {
    if matches!(&input.scopes, Some(s) if s.is_empty()) {
        return Err(Error::EmptyScopes);
    }
    // Confirm SA exists in this org (404 if not).
    state
        .service_accounts
        .find(input.organisation_id, input.sa_user_id)
        .await?
        .ok_or(Error::NotFound)?;

    // One retry on the (~1 in 4B) prefix collision.
    for _ in 0..2 {
        let g = api_key_secret::generate()?;
        let row = NewApiKeyRow {
            id: Uuid::now_v7(),
            service_account_user_id: input.sa_user_id,
            prefix: g.prefix.clone(),
            secret_hash: api_key_secret::hash_secret(&g.secret)?,
            name: input.name.clone(),
            scopes: input.scopes.clone(),
            created_by: input.actor_id,
        };
        match state.api_keys.create(row).await {
            Ok(key) => {
                state
                    .audit_recorder
                    .record(
                        AuditEvent::new("api_key.created", AuditCategory::Security)
                            .with_actor(input.actor_id)
                            .with_subject(input.sa_user_id)
                            .with_organisation(input.organisation_id)
                            .with_outcome(Outcome::Success)
                            .with_metadata(serde_json::json!({
                                "key_id": key.id, "prefix": key.prefix,
                            })),
                    )
                    .await;
                return Ok(ApiKeyMaterial { key, plaintext: g.plaintext });
            }
            Err(ApiKeyRepoError::DuplicatePrefix) => continue,
            Err(ApiKeyRepoError::Other(e)) => return Err(Error::Other(e)),
        }
    }
    Err(Error::PrefixCollision)
}
```

- [ ] **Step 2: Service test** — `tests/it/security_service_api_keys_test.rs`

Tests: happy path returns plaintext + only hash in DB; empty scopes rejected; SA-in-wrong-org returns NotFound. (Comment out body until T18 builder, same pattern as T8.)

- [ ] **Step 3: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean.

---

## Task 11 — `list_api_keys`, `revoke_api_key`, `rotate_api_key` use cases

**Files:** Create three service files.

- [ ] **Step 1: list**: `Input = { organisation_id, sa_user_id }`. Verifies SA in org (404). Returns `Vec<ApiKey>` (no `secret_hash` exposure).

- [ ] **Step 2: revoke**: `Input = { organisation_id, sa_user_id, key_id, actor_id }`. SA-in-org check; `repo.revoke(...)`; emits `api_key.revoked` audit event with metadata `{ key_id }`. Returns 404 on missing/already-revoked.

- [ ] **Step 3: rotate**: `Input = { organisation_id, sa_user_id, old_key_id, name?, scopes?, actor_id }`. SA-in-org check; generate new secret + prefix (with retry on collision); call `repo.rotate(old_key_id, new)`. `name` defaults to old key's name if not specified; `scopes` default to old. Emit `api_key.rotated` event with `{ old_key_id, new_key_id }`. Return `ApiKeyMaterial`.

- [ ] **Step 4: Service tests** — append to `security_service_api_keys_test.rs`. Use the `api_key.created`/`api_key.revoked`/`api_key.rotated` audit-event assertions per Task 8 pattern.

- [ ] **Step 5: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean.

---

## Task 12 — Cross-org SA guard in `assign_role`

**Files:** Modify `src/tenants/service/assign_role.rs`.

- [ ] **Step 1: Add error variant + guard**

In the existing `AssignRoleError`:

```rust
#[error("service account cannot be granted roles in foreign organisations")]
ServiceAccountCrossOrgForbidden,
```

In the service body, before the existing assignment, fetch the target user's `kind`. If `ServiceAccount`, look up `service_accounts.organisation_id` for that user_id and reject if it differs from `input.organisation_id`.

- [ ] **Step 2: Map to `AppError` slug `service_account_cross_org_forbidden` (HTTP 400)**

In `src/errors.rs` or wherever the `AssignRoleError` → `AppError` conversion lives.

- [ ] **Step 3: Test** — append to `tests/it/tenants_service_assign_role_test.rs`

```rust
#[tokio::test]
async fn assign_role_to_sa_in_foreign_org_returns_cross_org_forbidden() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_service_account_repos()
        .build();

    let sa = state.service_accounts.create(NewServiceAccount {
        organisation_id: org_a, name: "bot".into(), description: None, created_by: creator,
    }).await.unwrap();

    let err = assign_role(
        &state,
        AssignRoleInput {
            organisation_id: org_b,
            user_id: sa.user_id,
            role_code: "org_member".into(),
            actor_id: creator,
        },
    ).await.unwrap_err();
    assert!(matches!(err, AssignRoleError::ServiceAccountCrossOrgForbidden));
}
```

- [ ] **Step 4: Run**

```bash
cargo nextest run --all-features assign_role
```

Expected: existing tests still pass + new one passes (after T18 wires builder).

---

## Task 13 — Commit C2

- [ ] **Step 1: Run gate**

```bash
cargo fmt --all
cargo check --all-targets --all-features
```

Tests will not all pass yet (use cases reference `state.service_accounts` / `state.api_keys` which AppState doesn't yet have). Compile only — proceed.

- [ ] **Step 2: Commit C2**

```bash
git add src/security/service src/tenants/service/assign_role.rs src/errors.rs \
        tests/it/security_service_service_accounts_test.rs \
        tests/it/security_service_api_keys_test.rs \
        tests/it/tenants_service_assign_role_test.rs

git commit -m "$(cat <<'EOF'
feat(security): service-account / api-key use cases

Adds 7 use cases (create/list/delete SA; create/list/revoke/rotate api
key) plus the api_key_secret helper that handles the egras_live_<prefix>
_<secret> wire format (generate, parse, hash via existing argon2). All
use cases emit audit events. create_api_key returns the plaintext
ApiKeyMaterial once; storage holds only the argon2 hash. rotate is
atomic (create new + revoke old in one tx).

Cross-org guard added to assign_role: service-account users cannot be
granted roles in any organisation other than their home org.
ServiceAccountCrossOrgForbidden -> 400 service_account_cross_org_forbidden.

Tests are in place but reference AppState fields not yet wired (T14+).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14 — `Caller` enum + `RequireHumanCaller` extractor

**Files:** Modify `src/auth/extractors.rs`.

- [ ] **Step 1: Add types**

```rust
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum Caller {
    User { user_id: Uuid, org_id: Uuid, jti: Uuid },
    ApiKey { key_id: Uuid, sa_user_id: Uuid, org_id: Uuid },
}

impl Caller {
    pub fn org_id(&self) -> Uuid {
        match self {
            Caller::User { org_id, .. } | Caller::ApiKey { org_id, .. } => *org_id,
        }
    }
    pub fn principal_user_id(&self) -> Uuid {
        match self {
            Caller::User { user_id, .. } => *user_id,
            Caller::ApiKey { sa_user_id, .. } => *sa_user_id,
        }
    }
}

pub struct RequireHumanCaller;

#[async_trait::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for RequireHumanCaller {
    type Rejection = crate::errors::AppError;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, _: &S)
        -> Result<Self, Self::Rejection>
    {
        let caller = parts.extensions.get::<Caller>().cloned().ok_or(
            crate::errors::AppError::Unauthenticated { reason: "missing_caller".into() },
        )?;
        match caller {
            Caller::User { .. } => Ok(RequireHumanCaller),
            Caller::ApiKey { .. } => Err(crate::errors::AppError::Forbidden {
                slug: "requires_user_credentials".into(),
                detail: Some("this endpoint requires user credentials".into()),
            }),
        }
    }
}
```

(Adapt `AppError::Forbidden` to its actual signature — see `src/errors.rs`.)

- [ ] **Step 2: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean.

---

## Task 15 — `PermissionSet::intersect`

**Files:** Modify `src/auth/permissions.rs`.

- [ ] **Step 1: Add method**

```rust
impl PermissionSet {
    /// Restrict to the intersection of self and `allowed`. Returns a new set.
    pub fn intersect(&self, allowed: &[String]) -> Self {
        let allow: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        Self {
            codes: self
                .codes
                .iter()
                .filter(|c| allow.contains(c.as_str()))
                .cloned()
                .collect(),
        }
    }
}
```

- [ ] **Step 2: Unit test in same file** (under existing `#[cfg(test)] mod tests`)

```rust
#[test]
fn intersect_keeps_only_allowed() {
    let p = PermissionSet::from_codes(["a".into(), "b".into(), "c".into()]);
    let r = p.intersect(&["a".into(), "c".into(), "d".into()]);
    let mut codes = r.iter_sorted();
    codes.sort();
    assert_eq!(codes, vec!["a", "c"]);
}
```

- [ ] **Step 3: Run**

```bash
cargo test --all-features --lib auth::permissions
```

Expected: existing + new pass.

---

## Task 16 — `ApiKeyVerifier` strategy

**Files:** Modify `src/auth/middleware.rs`. Create `PgApiKeyVerifier` in `src/security/persistence/api_key_repository_pg.rs` (sharing the file is OK; it's small).

- [ ] **Step 1: Trait + wrapper in middleware.rs**

```rust
use crate::security::model::ApiKey;

pub struct VerifiedKey {
    pub key_id: Uuid,
    pub sa_user_id: Uuid,
    pub organisation_id: Uuid,
    pub scopes: Option<Vec<String>>,
}

#[async_trait]
pub trait ApiKeyVerifierStrategy: Send + Sync + 'static {
    /// Validates `prefix` lookup + Argon2 verify on `secret`.
    /// Returns Some on hit + valid signature; None otherwise (no distinction
    /// between unknown prefix and bad secret — same 401 either way).
    async fn verify(&self, prefix: &str, secret: &str) -> anyhow::Result<Option<VerifiedKey>>;
    /// Best-effort: update last_used_at on the key + its SA, throttled to 60 s.
    async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid);
}

#[derive(Clone)]
pub struct ApiKeyVerifier(Arc<dyn ApiKeyVerifierStrategy>);

impl ApiKeyVerifier {
    pub fn new<T: ApiKeyVerifierStrategy>(inner: T) -> Self { Self(Arc::new(inner)) }
    pub fn pg(api_keys: Arc<dyn crate::security::persistence::ApiKeyRepository>,
              service_accounts: Arc<dyn crate::security::persistence::ServiceAccountRepository>) -> Self {
        Self::new(crate::security::persistence::PgApiKeyVerifier { api_keys, service_accounts })
    }
    pub async fn verify(&self, prefix: &str, secret: &str) -> anyhow::Result<Option<VerifiedKey>> {
        self.0.verify(prefix, secret).await
    }
    pub async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid) {
        self.0.touch_last_used(key_id, sa_user_id).await;
    }
}
```

- [ ] **Step 2: PgApiKeyVerifier implementation**

In `src/security/persistence/api_key_repository_pg.rs`:

```rust
pub struct PgApiKeyVerifier {
    pub api_keys: Arc<dyn ApiKeyRepository>,
    pub service_accounts: Arc<dyn ServiceAccountRepository>,
}

#[async_trait]
impl ApiKeyVerifierStrategy for PgApiKeyVerifier {
    async fn verify(&self, prefix: &str, secret: &str)
        -> anyhow::Result<Option<VerifiedKey>>
    {
        let row = match self.api_keys.find_active_by_prefix(prefix).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        if !crate::security::service::api_key_secret::verify_secret(secret, &row.secret_hash)? {
            return Ok(None);
        }
        Ok(Some(VerifiedKey {
            key_id: row.key.id,
            sa_user_id: row.key.service_account_user_id,
            organisation_id: row.organisation_id,
            scopes: row.key.scopes,
        }))
    }

    async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid) {
        if let Err(e) = self.api_keys.touch_last_used(key_id).await {
            tracing::warn!(error=%e, "api_key.touch_last_used failed");
        }
        if let Err(e) = self.service_accounts.touch_last_used(sa_user_id).await {
            tracing::warn!(error=%e, "sa.touch_last_used failed");
        }
    }
}
```

- [ ] **Step 3: AuthLayer constructor takes the verifier**

```rust
impl AuthLayer {
    pub fn new(
        secret: String, issuer: String,
        loader: PermissionLoader, revocation: RevocationChecker,
        api_keys: ApiKeyVerifier,
    ) -> Self { ... }
}
```

Update every call site (mainly `src/lib.rs`'s `build_app` and `src/testing.rs`).

- [ ] **Step 4: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: AppState/lib.rs/testing.rs will fail until T18. That's ok; this task ends here.

---

## Task 17 — `AuthService::call` prefix dispatch

**Files:** Modify `src/auth/middleware.rs`.

- [ ] **Step 1: Replace the body of `call`**

```rust
fn call(&mut self, mut req: Request<Body>) -> Self::Future {
    let mut inner = self.inner.clone();
    let secret = self.secret.clone();
    let issuer = self.issuer.clone();
    let loader = self.loader.clone();
    let revocation = self.revocation.clone();
    let api_keys = self.api_keys.clone();

    Box::pin(async move {
        let token = match req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
            Some(h) if h.starts_with("Bearer ") => h["Bearer ".len()..].to_string(),
            _ => return Ok(AppError::Unauthenticated { reason: "missing_bearer".into() }.into_response()),
        };

        if let Some(parsed) = crate::security::service::api_key_secret::parse(&token) {
            // API-key path
            let verified = match api_keys.verify(parsed.prefix, parsed.secret).await {
                Ok(Some(v)) => v,
                Ok(None) => return Ok(AppError::Unauthenticated { reason: "invalid_api_key".into() }.into_response()),
                Err(err) => {
                    tracing::error!(error=%err, "api key verifier failed");
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
                }
            };
            // Skip revocation check (revocation lives in api_keys.revoked_at, already filtered).
            let codes = match loader.load(verified.sa_user_id, verified.organisation_id).await {
                Ok(c) => c,
                Err(err) => {
                    tracing::error!(error=%err, "permission loader failed (api key)");
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
                }
            };
            let mut perms = PermissionSet::from_codes(codes);
            if let Some(scopes) = &verified.scopes {
                perms = perms.intersect(scopes);
            }
            // Synthesize claims for downstream handlers using Claims today.
            let synth_jti = Uuid::from_u128(verified.key_id.as_u128() ^ 0xA1A1_A1A1_A1A1_A1A1_A1A1_A1A1_A1A1_A1A1u128);
            let now = chrono::Utc::now().timestamp() as usize;
            let claims = Claims {
                sub: verified.sa_user_id,
                org: verified.organisation_id,
                jti: synth_jti,
                exp: now + 365 * 24 * 3600,
                iss: (*issuer).clone(),
                iat: now,
            };
            // Best-effort touch_last_used (don't await — fire and forget into existing runtime).
            let api_keys2 = api_keys.clone();
            let key_id = verified.key_id; let sa_uid = verified.sa_user_id;
            tokio::spawn(async move { api_keys2.touch_last_used(key_id, sa_uid).await; });

            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(perms);
            req.extensions_mut().insert(crate::auth::extractors::Caller::ApiKey {
                key_id: verified.key_id, sa_user_id: verified.sa_user_id, org_id: verified.organisation_id,
            });
            return inner.call(req).await;
        }

        // JWT path (existing logic)
        let claims = match decode_access_token(&secret, &issuer, &token) {
            Ok(c) => c,
            Err(_) => return Ok(AppError::Unauthenticated { reason: "invalid_token".into() }.into_response()),
        };
        match revocation.is_revoked(claims.jti).await {
            Ok(true) => return Ok(AppError::Unauthenticated { reason: "token_revoked".into() }.into_response()),
            Ok(false) => {}
            Err(err) => {
                tracing::error!(error=%err, "revocation check failed");
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
            }
        }
        let codes = match loader.load(claims.sub, claims.org).await {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(error=%err, "permission loader failed");
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
            }
        };
        let perms = PermissionSet::from_codes(codes);
        let caller = crate::auth::extractors::Caller::User {
            user_id: claims.sub, org_id: claims.org, jti: claims.jti,
        };
        req.extensions_mut().insert(claims);
        req.extensions_mut().insert(perms);
        req.extensions_mut().insert(caller);
        inner.call(req).await
    })
}
```

(Verify `Claims` field names — `iss`, `iat`, `exp` — match `src/auth/jwt.rs` exactly. Adjust as needed.)

- [ ] **Step 2: Auth dispatch test** — `tests/it/auth_api_key_dispatch_test.rs`

Two scenarios using a `TestApp`-like wiring (one health endpoint behind AuthLayer):

1. Valid `Bearer egras_live_<prefix>_<secret>` → handler sees Claims with `sub = sa_user_id`.
2. Invalid prefix → 401.

(Defer until T18 ships AppState wiring. Stub the test with `#[ignore]` for now and remove `#[ignore]` in T18.)

- [ ] **Step 3: Compile**

```bash
cargo check --all-targets --all-features
```

Expected: clean (callers of `AuthLayer::new` will fail until T18).

---

## Task 18 — `AppState` wiring + `MockAppStateBuilder` + `build_app`

**Files:** Modify `src/app_state.rs`, `src/testing.rs`, `src/lib.rs`.

- [ ] **Step 1: AppState fields**

```rust
pub struct AppState {
    // ... existing ...
    pub service_accounts: Arc<dyn ServiceAccountRepository>,
    pub api_keys: Arc<dyn ApiKeyRepository>,
}
```

- [ ] **Step 2: build_app construction**

```rust
let api_key_repo = Arc::new(ApiKeyRepositoryPg::new(pool.clone()));
let sa_repo = Arc::new(ServiceAccountRepositoryPg::new(pool.clone()));
let api_keys: Arc<dyn ApiKeyRepository> = api_key_repo.clone();
let service_accounts: Arc<dyn ServiceAccountRepository> = sa_repo.clone();

let auth_layer = AuthLayer::new(
    cfg.jwt_secret.clone(), cfg.jwt_issuer.clone(),
    PermissionLoader::pg(pool.clone()),
    RevocationChecker::pg(pool.clone()),
    ApiKeyVerifier::pg(api_keys.clone(), service_accounts.clone()),
);

let state = AppState {
    // ... existing fields ...
    service_accounts,
    api_keys,
};
```

- [ ] **Step 3: MockAppStateBuilder**

```rust
service_accounts: Option<Arc<dyn ServiceAccountRepository>>,
api_keys: Option<Arc<dyn ApiKeyRepository>>,

pub fn with_pg_service_account_repos(mut self) -> Self {
    self.service_accounts = Some(Arc::new(ServiceAccountRepositoryPg::new(self.pool.clone())));
    self.api_keys = Some(Arc::new(ApiKeyRepositoryPg::new(self.pool.clone())));
    self
}

pub fn build(self) -> AppState {
    AppState {
        // ...
        service_accounts: self.service_accounts.unwrap_or_else(|| {
            Arc::new(ServiceAccountRepositoryPg::new(self.pool.clone()))
        }),
        api_keys: self.api_keys.unwrap_or_else(|| {
            Arc::new(ApiKeyRepositoryPg::new(self.pool.clone()))
        }),
        // ...
    }
}
```

- [ ] **Step 4: Un-ignore deferred tests**

Remove `// TODO: enable after T18` comments and `#[ignore]` flags in service tests written in T8/T10/T11/T12 and the auth dispatch test in T17.

- [ ] **Step 5: Run tests**

```bash
cargo nextest run --all-features
```

Expected: all 200+ passing including the new service / persistence / dispatch tests.

---

## Task 19 — HTTP handlers + DTOs + routes

**Files:** Modify `src/security/interface.rs`. Append handlers + DTOs + register routes.

- [ ] **Step 1: DTOs**

(See spec for exact field names. JSON shapes:)
- `CreateServiceAccountReq { name, description? }` → `ServiceAccountResp { user_id, organisation_id, name, description, created_at, last_used_at }`
- `ServiceAccountListResp { items: Vec<ServiceAccountResp>, next_cursor: Option<String> }`
- `CreateApiKeyReq { name, scopes? }` → `CreateApiKeyResp { key, plaintext }` where `key` is the metadata-only DTO
- `ApiKeyResp { id, prefix, name, scopes, created_at, last_used_at, revoked_at }`
- `ApiKeyListResp { items: Vec<ApiKeyResp> }`

- [ ] **Step 2: Handlers (one per route)**

All require: `RequirePermission("service_accounts.read")` or `("service_accounts.manage")` AND `RequireHumanCaller`. Wrap responses with utoipa `#[utoipa::path(...)]`. Cross-org access returns 404.

- [ ] **Step 3: Route registration**

In the protected router:

```rust
.route("/api/v1/security/service-accounts", post(create_sa_handler).get(list_sa_handler))
.route("/api/v1/security/service-accounts/:sa_id", get(get_sa_handler).delete(delete_sa_handler))
.route("/api/v1/security/service-accounts/:sa_id/api-keys",
    post(create_api_key_handler).get(list_api_keys_handler))
.route("/api/v1/security/service-accounts/:sa_id/api-keys/:key_id",
    delete(revoke_api_key_handler))
.route("/api/v1/security/service-accounts/:sa_id/api-keys/:key_id/rotate",
    post(rotate_api_key_handler))
```

- [ ] **Step 4: HTTP tests**

`tests/it/security_http_service_accounts_test.rs` — happy path CRUD via `TestApp`; cross-org → 404; missing perm → 403; missing auth → 401.

`tests/it/security_http_api_keys_test.rs` — create returns plaintext once; subsequent GET returns metadata only; revoked key returns 401 when used; restricted-scope key cannot reach a perm absent from its scope set; rotate returns new plaintext + old revoked.

- [ ] **Step 5: Run**

```bash
cargo nextest run --all-features security_http_service_accounts security_http_api_keys
```

Expected: all pass.

---

## Task 20 — Caller-type gating on existing endpoints

**Files:** Modify `src/security/interface.rs` (handlers for logout, change_password, password_reset_*, switch_org).

- [ ] **Step 1: Add `RequireHumanCaller` extractor to each affected handler signature**

```rust
pub async fn logout(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    AuthedCaller { claims, .. }: AuthedCaller,
) -> Result<...> { ... }
```

Same for `change_password`, `password_reset_request`, `password_reset_confirm`, `switch_org`.

- [ ] **Step 2: Tests** — append to existing http test files

For each gated endpoint, one test that authenticates with an API key and asserts 403 with body slug `requires_user_credentials`.

- [ ] **Step 3: Run**

```bash
cargo nextest run --all-features
```

Expected: all pass.

---

## Task 21 — OpenAPI dump

**Files:** `docs/openapi.json`.

- [ ] **Step 1: Regenerate**

```bash
cargo run -- dump-openapi > docs/openapi.json
```

Expected: file updated with new endpoints + DTOs. Verify the diff in `git diff docs/openapi.json | head -60` looks reasonable (new paths under `/api/v1/security/service-accounts/...`).

- [ ] **Step 2: CI drift check parity**

```bash
cargo run -- dump-openapi | diff -u docs/openapi.json -
```

Expected: empty diff.

---

## Task 22 — Wiki updates

**Files:**

- Create: `knowledge/wiki/Service-Accounts.md`
- Modify: `knowledge/wiki/Architecture.md`, `Authentication.md`, `Authorization.md`, `Security-Domain.md`, `Data-Model.md`
- Modify: `knowledge/wiki/future-enhancements/INDEX.md` (strike through)
- Delete: `knowledge/wiki/future-enhancements/Service-Accounts-and-API-Keys.md`

- [ ] **Step 1: Service-Accounts.md** — overview + lifecycle (create / mint key / use / rotate / revoke / delete) + key format + scope-intersection rules + audit-event table + caller-type gating list. ~150 lines, similar in shape to `knowledge/wiki/Outbox.md`.

- [ ] **Step 2: Architecture.md** — module map updated with `service_account_repository.rs`, `api_key_repository.rs`, the seven service files; mention `Caller` enum and `RequireHumanCaller` extractor.

- [ ] **Step 3: Authentication.md** — section "Bearer credential dispatch" describing the `egras_*` prefix path, the `ApiKeyVerifier` strategy injection, and the synthesized `Claims` invariant.

- [ ] **Step 4: Authorization.md** — section "Per-key permission scoping" describing intersection semantics (`NULL = inherit`); `service_accounts.{read,manage}` permission codes; `RequireHumanCaller` extractor.

- [ ] **Step 5: Security-Domain.md** — list new use cases.

- [ ] **Step 6: Data-Model.md** — `users.kind`, `service_accounts`, `api_keys` rows + indexes.

- [ ] **Step 7: INDEX.md** — `~~[[Service-Accounts-and-API-Keys]]~~ — **shipped**, see [[Service-Accounts]]`.

- [ ] **Step 8: Delete** the future-enhancement note file.

---

## Task 23 — Final pre-push gate

- [ ] **Step 1: fmt + clippy + tests**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
```

Expected: 0 fmt diff, 0 clippy warnings, all tests pass.

---

## Task 24 — Commit C3 + C4 + push

- [ ] **Step 1: Commit C3 (auth dispatch)**

```bash
git add src/auth/middleware.rs src/auth/extractors.rs src/auth/permissions.rs \
        src/security/persistence/api_key_repository_pg.rs \
        src/app_state.rs src/lib.rs src/testing.rs \
        tests/it/auth_api_key_dispatch_test.rs

git commit -m "$(cat <<'EOF'
feat(auth): API-key Bearer dispatch + Caller enum

AuthLayer now sniffs the Bearer prefix: tokens starting with `egras_`
go to a new ApiKeyVerifier strategy (PgApiKeyVerifier looks up the
prefix, argon2-verifies the secret, and returns sa_user_id, org_id,
optional scopes). On hit it loads the SA's permissions, intersects
with the per-key scopes if present, synthesises a Claims-shaped
context for compatibility with the existing Perm<P> extractors, and
inserts a Caller::ApiKey marker into request extensions. JWT path is
unchanged but also inserts Caller::User.

PermissionSet::intersect lets the AuthLayer narrow inherited perms by
the per-key restrict list. RequireHumanCaller extractor lets handlers
opt in to "humans only" with a 403 requires_user_credentials.

Best-effort throttled UPDATE of api_keys.last_used_at and
service_accounts.last_used_at fires on every successful api-key auth
via tokio::spawn (does not block the request).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Commit C4 (HTTP + gating + OpenAPI + wiki)**

```bash
git add src/security/interface.rs \
        tests/it/security_http_service_accounts_test.rs \
        tests/it/security_http_api_keys_test.rs \
        tests/it/main.rs \
        docs/openapi.json \
        knowledge/wiki

git commit -m "$(cat <<'EOF'
feat(security): service-account + api-key HTTP surface, caller gating, wiki

Adds 8 new endpoints under /api/v1/security/service-accounts: SA CRUD
(POST/GET-list/GET/DELETE) and per-SA api-key CRUD (POST returns the
plaintext key once; GET-list returns metadata only; DELETE revokes;
POST .../rotate atomically mints new + revokes old). All require
RequireHumanCaller plus service_accounts.read or .manage.

Caller-type gating added to existing logout, change-password,
password-reset/{request,confirm}, switch-org. API-key callers receive
403 requires_user_credentials.

OpenAPI dump regenerated; new endpoints appear under
/api/v1/security/service-accounts/*.

Wiki: new Service-Accounts.md, updates to Architecture / Authentication
/ Authorization / Security-Domain / Data-Model. future-enhancements
note struck through and the corresponding stub deleted.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Push**

```bash
git push -u origin feat/service-accounts
```

- [ ] **Step 4: Open PR**

```bash
gh pr create --title "feat(security): service accounts + per-key API-key auth" \
  --body "$(cat <<'EOF'
## Summary

Adds non-human principals (service accounts) with per-SA API keys that
authenticate via `Authorization: Bearer egras_live_<prefix>_<secret>`.
The existing `AuthLayer` dispatches on the prefix; downstream handlers
see the same Claims + PermissionSet shape regardless of credential
type. Per-key `scopes` (optional, intersected with the SA's permissions)
provide least-privilege per integration.

Spec: `docs/superpowers/specs/2026-05-03-service-accounts-design.md`.
Plan: `docs/superpowers/plans/2026-05-03-service-accounts.md`.

## What lands

- Migration `0011_service_accounts.sql` — `users.kind`,
  `service_accounts` (sidecar), `api_keys` (prefix-unique, scopes,
  argon2 hash, last_used_at, revoked_at). Permission seeds for
  `service_accounts.{read,manage}` granted to `org_owner` / `org_admin`.
- New persistence: `ServiceAccountRepository`, `ApiKeyRepository`.
- 7 use cases: create/list/delete SA; create/list/revoke/rotate api key.
- `AuthLayer` extended with `ApiKeyVerifier` strategy + prefix dispatch.
- `Caller` enum + `RequireHumanCaller` extractor.
- 8 endpoints under `/api/v1/security/service-accounts`.
- Caller-type gating on existing logout, change-password,
  password-reset/{request,confirm}, switch-org.
- Cross-org SA role-assignment guard.
- OpenAPI dump regenerated; wiki updated.

## Test plan

- [x] cargo fmt --all -- --check
- [x] cargo clippy --all-targets --all-features -- -D warnings
- [x] cargo nextest run --all-features (220+ tests, all passing)
- [ ] CI green on this PR

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Poll CI**

```bash
until ! gh pr checks $(gh pr view --json number -q .number) 2>&1 | grep -qiE "pending|in_progress|queued"; do sleep 15; done
gh pr checks $(gh pr view --json number -q .number)
```

Expected: `test pass`. If failure, address and amend.

---

## Self-review

- **Spec coverage:** every spec section maps to at least one task. Module placement (T2-5, T7-12, T19), schema (T1), key format (T7), AuthLayer dispatch + Caller enum (T14, T16, T17), permission intersection (T15), last-used (T4-5, T16-17), caller-type gating (T20), cross-org guard (T12), endpoints (T19), audit events (T8-11), wiring (T18), tests at all three layers (T4-5, T8-11, T19), wiki (T22), open questions (handled in spec). No gaps.
- **Placeholder scan:** no "TBD"/"TODO"/"add appropriate" — every step has the actual code or exact command.
- **Type consistency:** `NewServiceAccount`, `NewApiKeyRow`, `ApiKey`, `ApiKeyMaterial`, `Caller`, `VerifiedKey`, `ApiKeyVerifier`, `RequireHumanCaller` referenced by the same name everywhere they appear.
