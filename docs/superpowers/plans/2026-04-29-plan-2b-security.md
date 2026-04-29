# Plan 2b: Security Domain + Tenants Add/Remove Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the full security domain (7 endpoints) and 2 deferred tenants endpoints on the feat/security branch.

**Architecture:** Services are free functions on &AppState following existing tenants patterns. AppState gains UserRepository, TokenRepository, JwtConfig, and password_reset_ttl_secs. Security routes split between public (login, password-reset-*) and protected (register, logout, change-password, switch-org) sub-routers.

**Tech Stack:** axum 0.7, sqlx 0.8, argon2 0.5 (argon2id OWASP 2024 params), jsonwebtoken 9, sha2 0.10, rand 0.8, utoipa 4

---

## Task 1: Security models + UserRepository trait + TokenRepository trait

Establish the data model layer. No SQL yet — just structs and trait definitions. After this task the crate must compile.

- [ ] Create `src/security/model.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One entry per org the user belongs to, enriched for the login response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMembership {
    pub org_id: Uuid,
    pub org_name: String,
    pub role_codes: Vec<String>,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PasswordResetToken {
    pub id: Uuid,
    /// Hex-encoded SHA-256 of the raw token bytes — stored in DB.
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

- [ ] Create `src/security/persistence/user_repository.rs`:

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::{User, UserMembership};

#[derive(Debug, thiserror::Error)]
pub enum UserRepoError {
    #[error("duplicate username: {0}")]
    DuplicateUsername(String),
    #[error("duplicate email: {0}")]
    DuplicateEmail(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait UserRepository: Send + Sync + 'static {
    /// Insert a new user row. Returns the created User.
    async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<User, UserRepoError>;

    async fn find_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<User>, UserRepoError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, UserRepoError>;

    async fn update_password_hash(
        &self,
        user_id: Uuid,
        new_hash: &str,
    ) -> Result<(), UserRepoError>;

    /// Load all memberships for a user, ordered by joined_at DESC.
    async fn list_memberships(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserMembership>, UserRepoError>;
}
```

- [ ] Create `src/security/persistence/token_repository.rs`:

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::PasswordResetToken;

#[derive(Debug, thiserror::Error)]
pub enum TokenRepoError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait TokenRepository: Send + Sync + 'static {
    /// Insert a new pending token. Silently drops oldest if user already has
    /// MAX_PENDING_TOKENS_PER_USER (3) unexpired tokens.
    async fn insert(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<PasswordResetToken, TokenRepoError>;

    /// Find a token by its hash that is not yet used and not expired.
    async fn find_valid(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordResetToken>, TokenRepoError>;

    /// Mark the token as used (set used_at = now).
    async fn consume(&self, token_id: Uuid) -> Result<(), TokenRepoError>;
}
```

- [ ] Create `src/security/persistence/mod.rs`:

```rust
pub mod token_repository;
pub mod user_repository;

pub use token_repository::{TokenRepoError, TokenRepository};
pub use user_repository::{UserRepoError, UserRepository};
```

- [ ] Replace `src/security/mod.rs` with:

```rust
pub mod model;
pub mod persistence;
pub mod service;
pub mod interface;
```

- [ ] Create `src/security/service/mod.rs` (stub — real modules added per task):

```rust
pub mod change_password;
pub mod login;
pub mod logout;
pub mod password_reset_confirm;
pub mod password_reset_request;
pub mod register_user;
pub mod switch_org;
```

- [ ] Create stub files for each service module so `mod.rs` compiles. Each file contains only the public types; logic is `todo!()`. Example — create all 7 of these with the same pattern:

`src/security/service/login.rs`:
```rust
// Implemented in Task 4.
```

`src/security/service/logout.rs`:
```rust
// Implemented in Task 5.
```

`src/security/service/change_password.rs`:
```rust
// Implemented in Task 5.
```

`src/security/service/switch_org.rs`:
```rust
// Implemented in Task 5.
```

`src/security/service/register_user.rs`:
```rust
// Implemented in Task 4.
```

`src/security/service/password_reset_request.rs`:
```rust
// Implemented in Task 6.
```

`src/security/service/password_reset_confirm.rs`:
```rust
// Implemented in Task 6.
```

- [ ] Create `src/security/interface.rs` stub:

```rust
use axum::Router;
use crate::app_state::AppState;

pub fn public_router() -> Router<AppState> {
    Router::new()
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
}
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/security/
git commit -m "feat(security): add model, repository traits, and module stubs"
```

---

## Task 2: AppState + JwtConfig extension

Extend `AppState` and `JwtConfig`. After this task the full codebase must compile — stub PG impls are introduced so `build_app` can wire them.

- [ ] Add `JwtConfig` to `src/auth/jwt.rs` (append after existing code):

```rust
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub issuer: String,
    pub ttl_secs: i64,
}
```

- [ ] Edit `src/app_state.rs` — replace entire file:

```rust
use std::sync::Arc;

use sqlx::PgPool;

use crate::audit::service::{AuditRecorder, ListAuditEvents};
use crate::auth::jwt::JwtConfig;
use crate::security::persistence::{TokenRepository, UserRepository};
use crate::tenants::persistence::{OrganisationRepository, RoleRepository};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    pub organisations: Arc<dyn OrganisationRepository>,
    pub roles: Arc<dyn RoleRepository>,
    pub users: Arc<dyn UserRepository>,
    pub tokens: Arc<dyn TokenRepository>,
    pub jwt_config: JwtConfig,
    pub password_reset_ttl_secs: i64,
}
```

- [ ] Create `src/security/persistence/user_repository_pg.rs` (minimal — just enough to satisfy `Arc<dyn UserRepository>`):

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::user_repository::{UserRepoError, UserRepository};
use crate::security::model::{User, UserMembership};

pub struct UserRepositoryPg {
    pool: PgPool,
}

impl UserRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for UserRepositoryPg {
    async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<User, UserRepoError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, username, email, password_hash) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, username, email, password_hash, created_at, updated_at",
        )
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23505") {
                    if dbe.constraint() == Some("users_username_key") {
                        return UserRepoError::DuplicateUsername(username.to_string());
                    }
                    if dbe.constraint() == Some("users_email_key") {
                        return UserRepoError::DuplicateEmail(email.to_string());
                    }
                }
            }
            UserRepoError::Db(e)
        })?;
        Ok(row.into())
    }

    async fn find_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<User>, UserRepoError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, created_at, updated_at \
             FROM users WHERE username = $1 OR email = $1 LIMIT 1",
        )
        .bind(username_or_email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, UserRepoError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn update_password_hash(
        &self,
        user_id: Uuid,
        new_hash: &str,
    ) -> Result<(), UserRepoError> {
        sqlx::query(
            "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(new_hash)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_memberships(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserMembership>, UserRepoError> {
        let rows = sqlx::query_as::<_, MembershipRow>(
            "SELECT o.id AS org_id, o.name AS org_name, \
                    array_agg(DISTINCT r.code) AS role_codes, \
                    MIN(uor.created_at) AS joined_at \
             FROM user_organisation_roles uor \
             JOIN organisations o ON o.id = uor.organisation_id \
             JOIN roles r ON r.id = uor.role_id \
             WHERE uor.user_id = $1 \
             GROUP BY o.id, o.name \
             ORDER BY joined_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

// ── row structs ──────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    email: String,
    password_hash: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            username: r.username,
            email: r.email,
            password_hash: r.password_hash,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MembershipRow {
    org_id: Uuid,
    org_name: String,
    role_codes: Vec<String>,
    joined_at: DateTime<Utc>,
}

impl From<MembershipRow> for UserMembership {
    fn from(r: MembershipRow) -> Self {
        UserMembership {
            org_id: r.org_id,
            org_name: r.org_name,
            role_codes: r.role_codes,
            joined_at: r.joined_at,
        }
    }
}
```

- [ ] Create `src/security/persistence/token_repository_pg.rs`:

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::token_repository::{TokenRepoError, TokenRepository};
use crate::security::model::PasswordResetToken;

const MAX_PENDING_TOKENS_PER_USER: i64 = 3;

pub struct TokenRepositoryPg {
    pool: PgPool,
}

impl TokenRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TokenRepository for TokenRepositoryPg {
    async fn insert(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<PasswordResetToken, TokenRepoError> {
        let mut tx = self.pool.begin().await?;

        // Count existing valid tokens.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM password_reset_tokens \
             WHERE user_id = $1 AND used_at IS NULL AND expires_at > NOW()",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        // Silently delete the oldest when at capacity.
        if count >= MAX_PENDING_TOKENS_PER_USER {
            sqlx::query(
                "DELETE FROM password_reset_tokens \
                 WHERE id = ( \
                     SELECT id FROM password_reset_tokens \
                     WHERE user_id = $1 AND used_at IS NULL AND expires_at > NOW() \
                     ORDER BY created_at ASC LIMIT 1 \
                 )",
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }

        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, TokenRow>(
            "INSERT INTO password_reset_tokens \
             (id, user_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, user_id, token_hash, expires_at, used_at, created_at",
        )
        .bind(id)
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(row.into())
    }

    async fn find_valid(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordResetToken>, TokenRepoError> {
        let row = sqlx::query_as::<_, TokenRow>(
            "SELECT id, user_id, token_hash, expires_at, used_at, created_at \
             FROM password_reset_tokens \
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > NOW()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn consume(&self, token_id: Uuid) -> Result<(), TokenRepoError> {
        sqlx::query(
            "UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1",
        )
        .bind(token_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    user_id: Uuid,
    token_hash: String,
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<TokenRow> for PasswordResetToken {
    fn from(r: TokenRow) -> Self {
        PasswordResetToken {
            id: r.id,
            token_hash: r.token_hash,
            user_id: r.user_id,
            expires_at: r.expires_at,
            used_at: r.used_at,
            created_at: r.created_at,
        }
    }
}
```

- [ ] Update `src/security/persistence/mod.rs` to also re-export PG impls:

```rust
pub mod token_repository;
pub mod token_repository_pg;
pub mod user_repository;
pub mod user_repository_pg;

pub use token_repository::{TokenRepoError, TokenRepository};
pub use token_repository_pg::TokenRepositoryPg;
pub use user_repository::{UserRepoError, UserRepository};
pub use user_repository_pg::UserRepositoryPg;
```

- [ ] Wire security repos into `src/lib.rs` — in `build_app`, replace the `AppState { ... }` construction block:

```rust
    let users: Arc<dyn crate::security::persistence::UserRepository> = Arc::new(
        crate::security::persistence::UserRepositoryPg::new(pool.clone()),
    );
    let tokens: Arc<dyn crate::security::persistence::TokenRepository> = Arc::new(
        crate::security::persistence::TokenRepositoryPg::new(pool.clone()),
    );

    let state = AppState {
        pool: pool.clone(),
        audit_recorder,
        list_audit_events,
        organisations,
        roles,
        users,
        tokens,
        jwt_config: crate::auth::jwt::JwtConfig {
            secret: cfg.jwt_secret.clone(),
            issuer: cfg.jwt_issuer.clone(),
            ttl_secs: cfg.jwt_ttl_secs,
        },
        password_reset_ttl_secs: cfg.password_reset_ttl_secs,
    };
```

- [ ] Add `jwt_ttl_secs: i64` and `password_reset_ttl_secs: i64` to `AppConfig` in `src/config.rs`. Add defaults: `jwt_ttl_secs = 3600`, `password_reset_ttl_secs = 900`. Also update `AppConfig::default_for_tests()`.

- [ ] Extend `MockAppStateBuilder` in `src/testing.rs` with security fields:

```rust
// New fields in struct:
    users: Option<Arc<dyn crate::security::persistence::UserRepository>>,
    tokens: Option<Arc<dyn crate::security::persistence::TokenRepository>>,
    jwt_config: Option<crate::auth::jwt::JwtConfig>,
    password_reset_ttl_secs: Option<i64>,

// New setters:
    pub fn with_pg_security_repos(mut self) -> Self {
        self.users = Some(Arc::new(
            crate::security::persistence::UserRepositoryPg::new(self.pool.clone()),
        ));
        self.tokens = Some(Arc::new(
            crate::security::persistence::TokenRepositoryPg::new(self.pool.clone()),
        ));
        self
    }

    pub fn jwt_config(mut self, cfg: crate::auth::jwt::JwtConfig) -> Self {
        self.jwt_config = Some(cfg);
        self
    }

    pub fn users(mut self, r: Arc<dyn crate::security::persistence::UserRepository>) -> Self {
        self.users = Some(r);
        self
    }

    pub fn tokens(mut self, r: Arc<dyn crate::security::persistence::TokenRepository>) -> Self {
        self.tokens = Some(r);
        self
    }

// In build():
    AppState {
        pool: self.pool,
        audit_recorder: self.audit_recorder.expect("audit_recorder not set"),
        list_audit_events: self.list_audit_events.expect("list_audit_events not set"),
        organisations: self.organisations.expect("organisations not set"),
        roles: self.roles.expect("roles not set"),
        users: self.users.expect("users not set"),
        tokens: self.tokens.expect("tokens not set"),
        jwt_config: self.jwt_config.unwrap_or_else(|| crate::auth::jwt::JwtConfig {
            secret: "test-secret-32bytes-padding-here".to_string(),
            issuer: "egras-test".to_string(),
            ttl_secs: 3600,
        }),
        password_reset_ttl_secs: self.password_reset_ttl_secs.unwrap_or(900),
    }
```

- [ ] Add SQL migration `migrations/0006_password_reset_tokens.sql`:

```sql
CREATE TABLE password_reset_tokens (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    expires_at   TIMESTAMPTZ NOT NULL,
    used_at      TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX password_reset_tokens_user_id_idx
    ON password_reset_tokens(user_id)
    WHERE used_at IS NULL;
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/ migrations/
git commit -m "feat(security): extend AppState with UserRepository, TokenRepository, JwtConfig"
```

---

## Task 3: OrganisationRepository extensions (add_member / remove_member_checked)

- [ ] Add new `RepoError` variants and two new trait methods to `src/tenants/persistence/organisation_repository.rs`. Replace the `RepoError` enum and add methods to the trait:

```rust
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("duplicate organisation name: {0}")]
    DuplicateName(String),
    #[error("unknown role code: {0}")]
    UnknownRoleCode(String),
    #[error("unknown user: {0}")]
    UnknownUser(Uuid),
    #[error("organisation or user not found")]
    NotFound,
    #[error("user is not a member of the organisation")]
    NotMember,
    #[error("cannot remove the last owner of an organisation")]
    LastOwner,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}
```

Add to the `OrganisationRepository` trait (after `is_member`):

```rust
    /// Add a user to an org with the given role_code. Idempotent on the role row.
    async fn add_member(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        role_code: &str,
    ) -> Result<(), RepoError>;

    /// Remove all role rows for (user_id, org_id). Refuses with `LastOwner`
    /// if this would leave the org with zero org_owner rows. Uses FOR UPDATE.
    async fn remove_member_checked(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), RepoError>;
```

- [ ] Implement `add_member` and `remove_member_checked` in `src/tenants/persistence/organisation_repository_pg.rs`. Append after the `is_member` impl:

```rust
    async fn add_member(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        role_code: &str,
    ) -> Result<(), RepoError> {
        let role_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE code = $1",
        )
        .bind(role_code)
        .fetch_optional(&self.pool)
        .await?;

        let role_id = role_id.ok_or_else(|| RepoError::UnknownRoleCode(role_code.to_string()))?;

        sqlx::query(
            "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(org_id)
        .bind(role_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23503") {
                    return RepoError::NotFound;
                }
            }
            RepoError::Db(e)
        })?;

        Ok(())
    }

    async fn remove_member_checked(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await?;

        // Check membership exists.
        let is_member: bool = sqlx::query_scalar(
            "SELECT EXISTS( \
                 SELECT 1 FROM user_organisation_roles \
                 WHERE user_id = $1 AND organisation_id = $2 \
             ) FOR UPDATE",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;

        if !is_member {
            return Err(RepoError::NotMember);
        }

        // Count other org_owner members.
        let other_owners: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) \
             FROM user_organisation_roles uor \
             JOIN roles r ON r.id = uor.role_id \
             WHERE uor.organisation_id = $1 \
               AND r.code = 'org_owner' \
               AND uor.user_id != $2",
        )
        .bind(org_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        // Check if the target user holds the org_owner role.
        let target_is_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS( \
                 SELECT 1 FROM user_organisation_roles uor \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.user_id = $1 AND uor.organisation_id = $2 \
                   AND r.code = 'org_owner' \
             )",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;

        if target_is_owner && other_owners == 0 {
            return Err(RepoError::LastOwner);
        }

        sqlx::query(
            "DELETE FROM user_organisation_roles \
             WHERE user_id = $1 AND organisation_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/tenants/persistence/
git commit -m "feat(tenants): add add_member + remove_member_checked to OrganisationRepository"
```

---

## Task 4: Password hashing helpers + register_user + login services

- [ ] Create `src/security/service/password.rs` (shared hashing logic):

```rust
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params, Version,
};

/// OWASP 2024 recommended argon2id parameters.
const M_COST: u32 = 19_456;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

/// Argon2id hash tag expected in a correctly-parameterised hash string.
const EXPECTED_PARAMS_FRAGMENT: &str = "m=19456,t=2,p=1";

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(M_COST, T_COST, P_COST, None)
        .map_err(|e| anyhow::anyhow!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash_password: {e}"))?
        .to_string();
    Ok(hash)
}

/// Returns Ok(true) if the password matches. Returns Ok(false) on mismatch.
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("parse hash: {e}"))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(anyhow::anyhow!("verify_password: {e}")),
    }
}

/// Returns true if `hash` was produced with the current OWASP parameters.
pub fn needs_rehash(hash: &str) -> bool {
    !hash.contains(EXPECTED_PARAMS_FRAGMENT)
}
```

Add `pub mod password;` to `src/security/service/mod.rs`.

- [ ] Replace `src/security/service/register_user.rs` with full implementation:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::security::persistence::{UserRepoError, UserRepository};
use crate::tenants::persistence::RepoError as OrgRepoError;

#[derive(Debug, Clone)]
pub struct RegisterUserInput {
    pub username: String,
    pub email: String,
    pub password: String,
    pub target_org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterUserOutput {
    pub user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterUserError {
    #[error("username already taken")]
    DuplicateUsername,
    #[error("email already registered")]
    DuplicateEmail,
    #[error("invalid username: must be 1-64 non-empty ASCII chars")]
    InvalidUsername,
    #[error("invalid email")]
    InvalidEmail,
    #[error("password too short (min 8 chars)")]
    PasswordTooShort,
    #[error("organisation not found")]
    OrgNotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn register_user(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: RegisterUserInput,
) -> Result<RegisterUserOutput, RegisterUserError> {
    // Input validation.
    let username = input.username.trim().to_string();
    let email = input.email.trim().to_lowercase();
    if username.is_empty() || username.len() > 64 {
        return Err(RegisterUserError::InvalidUsername);
    }
    if !email.contains('@') || email.len() > 254 {
        return Err(RegisterUserError::InvalidEmail);
    }
    if input.password.len() < 8 {
        return Err(RegisterUserError::PasswordTooShort);
    }

    let hash = super::password::hash_password(&input.password)
        .map_err(RegisterUserError::Internal)?;

    let user = state
        .users
        .create(&username, &email, &hash)
        .await
        .map_err(|e| match e {
            UserRepoError::DuplicateUsername(_) => RegisterUserError::DuplicateUsername,
            UserRepoError::DuplicateEmail(_) => RegisterUserError::DuplicateEmail,
            UserRepoError::Db(e) => RegisterUserError::Internal(e.into()),
        })?;

    state
        .organisations
        .add_member(user.id, input.target_org_id, &input.role_code)
        .await
        .map_err(|e| match e {
            OrgRepoError::NotFound => RegisterUserError::OrgNotFound,
            OrgRepoError::UnknownRoleCode(_) => RegisterUserError::UnknownRoleCode,
            e => RegisterUserError::Internal(anyhow::anyhow!(e)),
        })?;

    let event = AuditEvent::user_registered_success(
        actor_user_id,
        actor_org_id,
        user.id,
        input.target_org_id,
        input.role_code,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user.id, "audit record failed for user.registered");
    }

    Ok(RegisterUserOutput { user_id: user.id })
}
```

- [ ] Replace `src/security/service/login.rs` with full implementation:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::auth::jwt::encode_access_token;
use crate::security::model::UserMembership;

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub username_or_email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct LoginOutput {
    pub token: String,
    pub user_id: Uuid,
    pub active_org_id: Uuid,
    pub memberships: Vec<UserMembership>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("user belongs to no organisation")]
    NoOrganisation,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn login(
    state: &AppState,
    input: LoginInput,
) -> Result<LoginOutput, LoginError> {
    let user = state
        .users
        .find_by_username_or_email(&input.username_or_email)
        .await
        .map_err(|e| LoginError::Internal(e.into()))?;

    let user = match user {
        Some(u) => u,
        None => {
            let event = AuditEvent::login_failed("not_found", &input.username_or_email);
            let _ = state.audit_recorder.record(event).await;
            return Err(LoginError::InvalidCredentials);
        }
    };

    let ok = super::password::verify_password(&input.password, &user.password_hash)
        .map_err(LoginError::Internal)?;
    if !ok {
        let event = AuditEvent::login_failed("bad_password", &input.username_or_email);
        let _ = state.audit_recorder.record(event).await;
        return Err(LoginError::InvalidCredentials);
    }

    // Opportunistic rehash.
    if super::password::needs_rehash(&user.password_hash) {
        if let Ok(new_hash) = super::password::hash_password(&input.password) {
            let _ = state.users.update_password_hash(user.id, &new_hash).await;
        }
    }

    let memberships = state
        .users
        .list_memberships(user.id)
        .await
        .map_err(|e| LoginError::Internal(e.into()))?;

    if memberships.is_empty() {
        return Err(LoginError::NoOrganisation);
    }

    let active_org_id = memberships[0].org_id;

    let token = encode_access_token(
        &state.jwt_config.secret,
        &state.jwt_config.issuer,
        user.id,
        active_org_id,
        state.jwt_config.ttl_secs,
    )
    .map_err(LoginError::Internal)?;

    let event = AuditEvent::login_success(user.id, active_org_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user.id, "audit record failed for login.success");
    }

    Ok(LoginOutput {
        token,
        user_id: user.id,
        active_org_id,
        memberships,
    })
}
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/security/service/
git commit -m "feat(security): implement register_user and login services"
```

---

## Task 5: logout + change_password + switch_org services

- [ ] Replace `src/security/service/logout.rs`:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, thiserror::Error)]
pub enum LogoutError {
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn logout(
    state: &AppState,
    user_id: Uuid,
    org_id: Uuid,
    jti: Uuid,
) -> Result<(), LogoutError> {
    let event = AuditEvent::logout(user_id, org_id, jti);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for logout");
    }
    Ok(())
}
```

- [ ] Replace `src/security/service/change_password.rs`:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct ChangePasswordInput {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ChangePasswordError {
    #[error("current password is incorrect")]
    WrongCurrentPassword,
    #[error("new password too short (min 8 chars)")]
    PasswordTooShort,
    #[error("user not found")]
    UserNotFound,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn change_password(
    state: &AppState,
    user_id: Uuid,
    input: ChangePasswordInput,
) -> Result<(), ChangePasswordError> {
    if input.new_password.len() < 8 {
        return Err(ChangePasswordError::PasswordTooShort);
    }

    let user = state
        .users
        .find_by_id(user_id)
        .await
        .map_err(|e| ChangePasswordError::Internal(e.into()))?
        .ok_or(ChangePasswordError::UserNotFound)?;

    let ok = super::password::verify_password(&input.current_password, &user.password_hash)
        .map_err(ChangePasswordError::Internal)?;
    if !ok {
        return Err(ChangePasswordError::WrongCurrentPassword);
    }

    let new_hash = super::password::hash_password(&input.new_password)
        .map_err(ChangePasswordError::Internal)?;

    state
        .users
        .update_password_hash(user_id, &new_hash)
        .await
        .map_err(|e| ChangePasswordError::Internal(e.into()))?;

    let event = AuditEvent::password_changed(user_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for password.changed");
    }

    Ok(())
}
```

- [ ] Replace `src/security/service/switch_org.rs`:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::auth::jwt::encode_access_token;

#[derive(Debug, Clone)]
pub struct SwitchOrgInput {
    pub target_org_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct SwitchOrgOutput {
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SwitchOrgError {
    #[error("user is not a member of the target organisation")]
    NotMember,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn switch_org(
    state: &AppState,
    user_id: Uuid,
    current_org_id: Uuid,
    input: SwitchOrgInput,
) -> Result<SwitchOrgOutput, SwitchOrgError> {
    let is_member = state
        .organisations
        .is_member(user_id, input.target_org_id)
        .await
        .map_err(|e| SwitchOrgError::Internal(anyhow::anyhow!(e)))?;

    if !is_member {
        return Err(SwitchOrgError::NotMember);
    }

    let token = encode_access_token(
        &state.jwt_config.secret,
        &state.jwt_config.issuer,
        user_id,
        input.target_org_id,
        state.jwt_config.ttl_secs,
    )
    .map_err(SwitchOrgError::Internal)?;

    let event = AuditEvent::session_switched_org(user_id, current_org_id, input.target_org_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for session.switched_org");
    }

    Ok(SwitchOrgOutput { token })
}
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/security/service/
git commit -m "feat(security): implement logout, change_password, switch_org services"
```

---

## Task 6: password_reset_request + password_reset_confirm services

- [ ] Replace `src/security/service/password_reset_request.rs`:

```rust
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct PasswordResetRequestInput {
    pub email: String,
    /// Base URL used when constructing the reset link logged at INFO.
    pub base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PasswordResetRequestError {
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn password_reset_request(
    state: &AppState,
    input: PasswordResetRequestInput,
) -> Result<(), PasswordResetRequestError> {
    // Always emit audit so timing is uniform.
    let event = AuditEvent::password_reset_requested(&input.email);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for password.reset_requested");
    }

    let user = state
        .users
        .find_by_username_or_email(&input.email)
        .await
        .map_err(|e| PasswordResetRequestError::Internal(e.into()))?;

    let Some(user) = user else {
        // Return success silently — do not leak whether the email exists.
        return Ok(());
    };

    // Generate 32 random bytes; raw = hex string; stored = SHA-256(raw).
    let mut raw_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut raw_bytes);
    let raw_hex = hex::encode(raw_bytes);
    let token_hash = hex::encode(Sha256::digest(raw_bytes));

    use rand::RngCore;

    let expires_at = chrono::Utc::now()
        + chrono::Duration::seconds(state.password_reset_ttl_secs);

    state
        .tokens
        .insert(user.id, &token_hash, expires_at)
        .await
        .map_err(|e| PasswordResetRequestError::Internal(e.into()))?;

    // Log reset URL at INFO (email delivery is out of scope for this seed).
    info!(
        user_id = %user.id,
        reset_url = %format!("{}/reset-password?token={}", input.base_url.trim_end_matches('/'), raw_hex),
        "password reset token issued",
    );

    Ok(())
}
```

Note: fix the `rand::RngCore` import — move it to the top of the file alongside the other imports:

```rust
use rand::RngCore;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
```

And replace `rand::thread_rng().fill(&mut raw_bytes)` with `rand::thread_rng().fill_bytes(&mut raw_bytes)`.

- [ ] Replace `src/security/service/password_reset_confirm.rs`:

```rust
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::app_state::AppState;
use crate::audit::model::{AuditEvent, Outcome};

#[derive(Debug, Clone)]
pub struct PasswordResetConfirmInput {
    /// The raw hex token from the URL (unhashed).
    pub raw_token: String,
    pub new_password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PasswordResetConfirmError {
    #[error("token is invalid or expired")]
    InvalidToken,
    #[error("new password too short (min 8 chars)")]
    PasswordTooShort,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn password_reset_confirm(
    state: &AppState,
    input: PasswordResetConfirmInput,
) -> Result<(), PasswordResetConfirmError> {
    if input.new_password.len() < 8 {
        return Err(PasswordResetConfirmError::PasswordTooShort);
    }

    // Decode raw token bytes and hash them.
    let raw_bytes = hex::decode(&input.raw_token)
        .map_err(|_| PasswordResetConfirmError::InvalidToken)?;
    let token_hash = hex::encode(Sha256::digest(&raw_bytes));

    let token = state
        .tokens
        .find_valid(&token_hash)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    let Some(token) = token else {
        let event = AuditEvent::password_reset_confirmed(
            None,
            Outcome::Failure,
            Some("invalid_token".into()),
        );
        let _ = state.audit_recorder.record(event).await;
        return Err(PasswordResetConfirmError::InvalidToken);
    };

    let new_hash = super::password::hash_password(&input.new_password)
        .map_err(PasswordResetConfirmError::Internal)?;

    state
        .users
        .update_password_hash(token.user_id, &new_hash)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    state
        .tokens
        .consume(token.id)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    let event =
        AuditEvent::password_reset_confirmed(Some(token.user_id), Outcome::Success, None);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %token.user_id, "audit record failed for password.reset_confirmed");
    }

    Ok(())
}
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/security/service/
git commit -m "feat(security): implement password_reset_request and password_reset_confirm services"
```

---

## Task 7: Tenants add_user + remove_user_from_organisation services

- [ ] Create `src/tenants/service/add_user_to_organisation.rs`:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct AddUserToOrganisationInput {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AddUserToOrganisationError {
    #[error("organisation or user not found")]
    NotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn add_user_to_organisation(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: AddUserToOrganisationInput,
) -> Result<(), AddUserToOrganisationError> {
    state
        .organisations
        .add_member(input.user_id, input.org_id, &input.role_code)
        .await
        .map_err(|e| match e {
            RepoError::NotFound => AddUserToOrganisationError::NotFound,
            RepoError::UnknownRoleCode(_) => AddUserToOrganisationError::UnknownRoleCode,
            e => AddUserToOrganisationError::Repo(e),
        })?;

    let event = AuditEvent::organisation_member_added(
        actor_user_id,
        actor_org_id,
        input.org_id,
        input.user_id,
        &input.role_code,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for organisation.member_added");
    }

    Ok(())
}
```

- [ ] Create `src/tenants/service/remove_user_from_organisation.rs`:

```rust
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct RemoveUserFromOrganisationInput {
    pub user_id: Uuid,
    pub org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveUserFromOrganisationError {
    #[error("user is not a member of the organisation")]
    NotMember,
    #[error("cannot remove the last owner of an organisation")]
    LastOwner,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn remove_user_from_organisation(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: RemoveUserFromOrganisationInput,
) -> Result<(), RemoveUserFromOrganisationError> {
    state
        .organisations
        .remove_member_checked(input.user_id, input.org_id)
        .await
        .map_err(|e| match e {
            RepoError::NotMember => RemoveUserFromOrganisationError::NotMember,
            RepoError::LastOwner => RemoveUserFromOrganisationError::LastOwner,
            e => RemoveUserFromOrganisationError::Repo(e),
        })?;

    let event = AuditEvent::organisation_member_removed(
        actor_user_id,
        actor_org_id,
        input.org_id,
        input.user_id,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for organisation.member_removed");
    }

    Ok(())
}
```

- [ ] Add both modules to `src/tenants/service/mod.rs`:

```rust
pub mod add_user_to_organisation;
pub mod assign_role;
pub mod create_organisation;
pub mod cursor_codec;
pub mod list_my_organisations;
pub mod list_organisation_members;
pub mod remove_user_from_organisation;
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -40
```

- [ ] Commit:

```bash
git add src/tenants/service/
git commit -m "feat(tenants): implement add_user_to_organisation and remove_user_from_organisation services"
```

---

## Task 8: Permission markers + HTTP interface for security + tenants add/remove

- [ ] Add permission markers to `src/auth/extractors.rs` (append after `TenantsRolesAssign`):

```rust
/// Permission marker: `users.manage_all` — platform-level user administration.
pub struct UsersManageAll;
impl Permission for UsersManageAll {
    const CODE: &'static str = "users.manage_all";
}

/// Permission marker: `tenants.members.add`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct TenantsMembersAdd;
impl Permission for TenantsMembersAdd {
    const CODE: &'static str = "tenants.members.add";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: `tenants.members.remove`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct TenantsMembersRemove;
impl Permission for TenantsMembersRemove {
    const CODE: &'static str = "tenants.members.remove";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}
```

- [ ] Replace `src/security/interface.rs` with full implementation:

```rust
use std::collections::HashMap;

use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::auth::extractors::{AuthedCaller, Perm, TenantsMembersAdd, UsersManageAll};
use crate::errors::AppError;
use crate::security::model::UserMembership;
use crate::security::service::change_password::{
    change_password, ChangePasswordError, ChangePasswordInput,
};
use crate::security::service::login::{login, LoginError, LoginInput};
use crate::security::service::logout::{logout, LogoutError};
use crate::security::service::password_reset_confirm::{
    password_reset_confirm, PasswordResetConfirmError, PasswordResetConfirmInput,
};
use crate::security::service::password_reset_request::{
    password_reset_request, PasswordResetRequestError, PasswordResetRequestInput,
};
use crate::security::service::register_user::{
    register_user, RegisterUserError, RegisterUserInput,
};
use crate::security::service::switch_org::{switch_org, SwitchOrgError, SwitchOrgInput};

// ── Routers ──────────────────────────────────────────────────────────────────

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/login", post(post_login))
        .route("/password-reset-request", post(post_password_reset_request))
        .route("/password-reset-confirm", post(post_password_reset_confirm))
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/register", post(post_register))
        .route("/logout", post(post_logout))
        .route("/change-password", post(post_change_password))
        .route("/switch-org", post(post_switch_org))
}

// ── Request / Response bodies ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    pub user_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username_or_email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MembershipDto {
    pub org_id: Uuid,
    pub org_name: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: Uuid,
    pub active_org_id: Uuid,
    pub memberships: Vec<MembershipDto>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SwitchOrgRequest {
    pub org_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordResetRequestBody {
    pub email: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordResetConfirmBody {
    pub token: String,
    pub new_password: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/v1/security/register",
    tag = "security",
    request_body = RegisterRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "User registered", body = RegisterResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 409, description = "Duplicate username or email", body = ErrorBody),
    ),
)]
pub async fn post_register(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersAdd>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    let out = register_user(
        &state,
        caller.claims.sub,
        caller.claims.org,
        RegisterUserInput {
            username: req.username,
            email: req.email,
            password: req.password,
            target_org_id: req.org_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(|e| match e {
        RegisterUserError::DuplicateUsername => AppError::Conflict {
            reason: "username already taken".into(),
        },
        RegisterUserError::DuplicateEmail => AppError::Conflict {
            reason: "email already registered".into(),
        },
        RegisterUserError::InvalidUsername => field_error("username", "invalid"),
        RegisterUserError::InvalidEmail => field_error("email", "invalid"),
        RegisterUserError::PasswordTooShort => field_error("password", "too_short"),
        RegisterUserError::OrgNotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        RegisterUserError::UnknownRoleCode => field_error("role_code", "unknown_role_code"),
        RegisterUserError::Internal(e) => AppError::Internal(e),
    })?;

    Ok((StatusCode::CREATED, Json(RegisterResponse { user_id: out.user_id })))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/login",
    tag = "security",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials", body = ErrorBody),
    ),
)]
pub async fn post_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let out = login(
        &state,
        LoginInput {
            username_or_email: req.username_or_email,
            password: req.password,
        },
    )
    .await
    .map_err(|e| match e {
        LoginError::InvalidCredentials => AppError::InvalidCredentials,
        LoginError::NoOrganisation => AppError::UserNoOrganisation,
        LoginError::Internal(e) => AppError::Internal(e),
    })?;

    Ok(Json(LoginResponse {
        token: out.token,
        user_id: out.user_id,
        active_org_id: out.active_org_id,
        memberships: out
            .memberships
            .into_iter()
            .map(|m: UserMembership| MembershipDto {
                org_id: m.org_id,
                org_name: m.org_name,
                role_codes: m.role_codes,
            })
            .collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/logout",
    tag = "security",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Logged out"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
    ),
)]
pub async fn post_logout(
    State(state): State<AppState>,
    caller: AuthedCaller,
) -> Result<StatusCode, AppError> {
    logout(&state, caller.claims.sub, caller.claims.org, caller.claims.jti)
        .await
        .map_err(|LogoutError::Internal(e)| AppError::Internal(e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/change-password",
    tag = "security",
    request_body = ChangePasswordRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated or wrong current password", body = ErrorBody),
    ),
)]
pub async fn post_change_password(
    State(state): State<AppState>,
    caller: AuthedCaller,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    change_password(
        &state,
        caller.claims.sub,
        ChangePasswordInput {
            current_password: req.current_password,
            new_password: req.new_password,
        },
    )
    .await
    .map_err(|e| match e {
        ChangePasswordError::WrongCurrentPassword => AppError::InvalidCredentials,
        ChangePasswordError::PasswordTooShort => field_error("new_password", "too_short"),
        ChangePasswordError::UserNotFound => AppError::NotFound {
            resource: "user".into(),
        },
        ChangePasswordError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/switch-org",
    tag = "security",
    request_body = SwitchOrgRequest,
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Org switched — new JWT", body = TokenResponse),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Not a member of target org", body = ErrorBody),
    ),
)]
pub async fn post_switch_org(
    State(state): State<AppState>,
    caller: AuthedCaller,
    Json(req): Json<SwitchOrgRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let out = switch_org(
        &state,
        caller.claims.sub,
        caller.claims.org,
        SwitchOrgInput {
            target_org_id: req.org_id,
        },
    )
    .await
    .map_err(|e| match e {
        SwitchOrgError::NotMember => AppError::PermissionDenied {
            code: "not_member".into(),
        },
        SwitchOrgError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(Json(TokenResponse { token: out.token }))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/password-reset-request",
    tag = "security",
    request_body = PasswordResetRequestBody,
    responses(
        (status = 204, description = "Reset email dispatched (always)"),
    ),
)]
pub async fn post_password_reset_request(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetRequestBody>,
) -> Result<StatusCode, AppError> {
    // Extract base URL from environment or use a safe fallback.
    let base_url = std::env::var("APP_BASE_URL")
        .unwrap_or_else(|_| "https://example.com".to_string());
    password_reset_request(
        &state,
        PasswordResetRequestInput {
            email: req.email,
            base_url,
        },
    )
    .await
    .map_err(|PasswordResetRequestError::Internal(e)| AppError::Internal(e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/password-reset-confirm",
    tag = "security",
    request_body = PasswordResetConfirmBody,
    responses(
        (status = 204, description = "Password reset"),
        (status = 400, description = "Token invalid or expired", body = ErrorBody),
    ),
)]
pub async fn post_password_reset_confirm(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetConfirmBody>,
) -> Result<StatusCode, AppError> {
    password_reset_confirm(
        &state,
        PasswordResetConfirmInput {
            raw_token: req.token,
            new_password: req.new_password,
        },
    )
    .await
    .map_err(|e| match e {
        PasswordResetConfirmError::InvalidToken => AppError::InvalidCredentials,
        PasswordResetConfirmError::PasswordTooShort => field_error("new_password", "too_short"),
        PasswordResetConfirmError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn field_error(field: &str, code: &str) -> AppError {
    let mut errs = HashMap::new();
    errs.insert(field.to_string(), vec![code.to_string()]);
    AppError::Validation { errors: errs }
}
```

- [ ] Add the two new tenants routes to `src/tenants/interface.rs`. Add to the `router()` function:

```rust
        .route("/add-user-to-organisation", post(post_add_user_to_organisation))
        .route("/remove-user-from-organisation", post(post_remove_user_from_organisation))
```

Add imports:
```rust
use crate::auth::extractors::{TenantsMembersAdd, TenantsMembersRemove};
use crate::tenants::service::add_user_to_organisation::{
    add_user_to_organisation, AddUserToOrganisationError, AddUserToOrganisationInput,
};
use crate::tenants::service::remove_user_from_organisation::{
    remove_user_from_organisation, RemoveUserFromOrganisationError,
    RemoveUserFromOrganisationInput,
};
```

Add handler functions:

```rust
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddUserToOrganisationRequest {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub role_code: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/tenants/add-user-to-organisation",
    tag = "tenants",
    request_body = AddUserToOrganisationRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "User added to organisation"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation or user not found", body = ErrorBody),
    ),
)]
pub async fn post_add_user_to_organisation(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersAdd>,
    Json(req): Json<AddUserToOrganisationRequest>,
) -> Result<StatusCode, AppError> {
    add_user_to_organisation(
        &state,
        caller.claims.sub,
        caller.claims.org,
        AddUserToOrganisationInput {
            user_id: req.user_id,
            org_id: req.org_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(|e| match e {
        AddUserToOrganisationError::NotFound => AppError::NotFound {
            resource: "organisation or user".into(),
        },
        AddUserToOrganisationError::UnknownRoleCode => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("role_code".into(), vec!["unknown_role_code".into()]);
            AppError::Validation { errors: errs }
        }
        AddUserToOrganisationError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
        AddUserToOrganisationError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RemoveUserFromOrganisationRequest {
    pub user_id: Uuid,
    pub org_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/v1/tenants/remove-user-from-organisation",
    tag = "tenants",
    request_body = RemoveUserFromOrganisationRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "User removed"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "User is not a member", body = ErrorBody),
        (status = 409, description = "Cannot remove last owner", body = ErrorBody),
    ),
)]
pub async fn post_remove_user_from_organisation(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersRemove>,
    Json(req): Json<RemoveUserFromOrganisationRequest>,
) -> Result<StatusCode, AppError> {
    remove_user_from_organisation(
        &state,
        caller.claims.sub,
        caller.claims.org,
        RemoveUserFromOrganisationInput {
            user_id: req.user_id,
            org_id: req.org_id,
        },
    )
    .await
    .map_err(|e| match e {
        RemoveUserFromOrganisationError::NotMember => AppError::NotFound {
            resource: "membership".into(),
        },
        RemoveUserFromOrganisationError::LastOwner => AppError::Conflict {
            reason: "cannot remove the last owner".into(),
        },
        RemoveUserFromOrganisationError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
        RemoveUserFromOrganisationError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] Wire security routers into `src/lib.rs`. In `build_app`, modify the public and protected router construction:

```rust
    // 2. Public routes (no auth)
    let public = Router::<AppState>::new()
        .route("/health", get(health))
        .route("/ready", get({ let pool = pool.clone(); move || ready(pool.clone()) }))
        .nest("/api/v1/security", crate::security::interface::public_router())
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/api-docs/openapi.json", crate::openapi::ApiDoc::openapi()),
        );

    // 3. Protected routes
    let protected: Router<AppState> = Router::new()
        .nest("/api/v1/tenants", crate::tenants::interface::router())
        .nest("/api/v1/security", crate::security::interface::protected_router())
        .layer(auth_layer);
```

- [ ] Verify:

```bash
cargo check --workspace 2>&1 | head -60
```

- [ ] Commit:

```bash
git add src/
git commit -m "feat(security): add HTTP interface for all security endpoints + tenants add/remove"
```

---

## Task 9: OpenAPI registration

- [ ] Replace `src/openapi.rs` with extended version that includes all new paths and schemas:

```rust
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "egras API",
        version = "0.1.0",
        description = "Enterprise-ready Rust application seed — tenants, security & audit",
    ),
    paths(
        crate::tenants::interface::post_create_organisation,
        crate::tenants::interface::get_list_my_organisations,
        crate::tenants::interface::get_list_members,
        crate::tenants::interface::post_assign_role,
        crate::tenants::interface::post_add_user_to_organisation,
        crate::tenants::interface::post_remove_user_from_organisation,
        crate::security::interface::post_register,
        crate::security::interface::post_login,
        crate::security::interface::post_logout,
        crate::security::interface::post_change_password,
        crate::security::interface::post_switch_org,
        crate::security::interface::post_password_reset_request,
        crate::security::interface::post_password_reset_confirm,
    ),
    components(
        schemas(
            crate::tenants::interface::CreateOrganisationRequest,
            crate::tenants::interface::OrganisationBody,
            crate::tenants::interface::PagedOrganisations,
            crate::tenants::interface::MemberBody,
            crate::tenants::interface::PagedMembers,
            crate::tenants::interface::AssignRoleRequest,
            crate::tenants::interface::AssignRoleResponseBody,
            crate::tenants::interface::AddUserToOrganisationRequest,
            crate::tenants::interface::RemoveUserFromOrganisationRequest,
            crate::security::interface::RegisterRequest,
            crate::security::interface::RegisterResponse,
            crate::security::interface::LoginRequest,
            crate::security::interface::LoginResponse,
            crate::security::interface::MembershipDto,
            crate::security::interface::ChangePasswordRequest,
            crate::security::interface::SwitchOrgRequest,
            crate::security::interface::TokenResponse,
            crate::security::interface::PasswordResetRequestBody,
            crate::security::interface::PasswordResetConfirmBody,
            crate::errors::ErrorBody,
        ),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "tenants", description = "Organisation and role management"),
        (name = "security", description = "Authentication and user management"),
    ),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .as_mut()
            .expect("components always set by derive");
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}
```

- [ ] Verify full build:

```bash
cargo build 2>&1 | head -60
```

- [ ] Commit:

```bash
git add src/openapi.rs
git commit -m "docs(openapi): register all security and tenants endpoints in OpenAPI spec"
```

---

## Task 10: Tests — persistence, service, and HTTP layers

### 10a — Test helpers

- [ ] Add `seed_user_with_password` to `tests/common/seed.rs`:

```rust
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, Params, Version,
};

/// Insert a user with a real argon2id hash so login tests can authenticate.
pub async fn seed_user_with_password(pool: &PgPool, username: &str, password: &str) -> Uuid {
    let id = Uuid::now_v7();
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(19_456, 2, 1, None).expect("argon2 params");
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("hash password")
        .to_string();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(username)
    .bind(format!("{username}@test"))
    .bind(hash)
    .execute(pool)
    .await
    .expect("seed user with password");
    id
}
```

### 10b — Persistence tests

- [ ] Create `tests/security_persistence_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::security::persistence::{TokenRepository, UserRepository};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::{seed_org, seed_user, grant_role};

#[tokio::test]
async fn user_repository_create_and_find() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let user = state
        .users
        .create("testuser", "testuser@example.com", "hash")
        .await
        .expect("create user");

    assert_eq!(user.username, "testuser");

    let found = state
        .users
        .find_by_username_or_email("testuser")
        .await
        .expect("find by username")
        .expect("should exist");
    assert_eq!(found.id, user.id);
}

#[tokio::test]
async fn user_repository_duplicate_username_is_error() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    state.users.create("dupe", "dupe1@example.com", "h").await.unwrap();
    let err = state.users.create("dupe", "dupe2@example.com", "h").await.unwrap_err();
    assert!(matches!(
        err,
        egras::security::persistence::UserRepoError::DuplicateUsername(_)
    ));
}

#[tokio::test]
async fn list_memberships_returns_orgs() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "memuser").await;
    let org = seed_org(&pool, "memorg", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let memberships = state.users.list_memberships(user).await.expect("list");
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0].org_id, org);
}

#[tokio::test]
async fn token_repository_insert_find_consume() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "tokuser").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(900);
    let tok = state
        .tokens
        .insert(user, "deadbeef_hash", expires_at)
        .await
        .expect("insert");

    let found = state
        .tokens
        .find_valid("deadbeef_hash")
        .await
        .expect("find")
        .expect("should exist");
    assert_eq!(found.id, tok.id);

    state.tokens.consume(tok.id).await.expect("consume");

    let gone = state
        .tokens
        .find_valid("deadbeef_hash")
        .await
        .expect("find after consume");
    assert!(gone.is_none());
}

#[tokio::test]
async fn token_repository_drops_oldest_when_at_capacity() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "capuser").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let exp = chrono::Utc::now() + chrono::Duration::seconds(900);
    for i in 0..3i32 {
        state
            .tokens
            .insert(user, &format!("hash_{i}"), exp)
            .await
            .expect("insert");
    }
    // 4th insertion should silently drop oldest.
    state.tokens.insert(user, "hash_new", exp).await.expect("4th insert");

    // hash_0 (oldest) should be gone; hash_new should exist.
    assert!(state.tokens.find_valid("hash_0").await.unwrap().is_none());
    assert!(state.tokens.find_valid("hash_new").await.unwrap().is_some());
}
```

### 10c — Service tests

- [ ] Create `tests/security_service_register_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::security::service::register_user::{register_user, RegisterUserError, RegisterUserInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::{seed_org, seed_user};

#[tokio::test]
async fn register_happy_path_creates_user_and_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin").await;
    let org = seed_org(&pool, "acme", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = register_user(
        &state,
        actor,
        org,
        RegisterUserInput {
            username: "newuser".into(),
            email: "newuser@example.com".into(),
            password: "password123".into(),
            target_org_id: org,
            role_code: "org_member".into(),
        },
    )
    .await
    .expect("register");

    let user = state.users.find_by_id(out.user_id).await.unwrap().unwrap();
    assert_eq!(user.username, "newuser");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'user.registered'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn register_duplicate_username_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin2").await;
    let org = seed_org(&pool, "acme2", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let input = || RegisterUserInput {
        username: "dupuser".into(),
        email: "dup@example.com".into(),
        password: "password123".into(),
        target_org_id: org,
        role_code: "org_member".into(),
    };

    register_user(&state, actor, org, input()).await.unwrap();

    let err = register_user(&state, actor, org, RegisterUserInput {
        email: "other@example.com".into(),
        ..input()
    })
    .await
    .unwrap_err();
    assert!(matches!(err, RegisterUserError::DuplicateUsername));
}
```

- [ ] Create `tests/security_service_login_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::security::service::login::{login, LoginError, LoginInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::{seed_org, seed_user_with_password, grant_role};

#[tokio::test]
async fn login_happy_path_returns_token_and_memberships() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "alice", "hunter2").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = login(
        &state,
        LoginInput {
            username_or_email: "alice".into(),
            password: "hunter2".into(),
        },
    )
    .await
    .expect("login");

    assert!(!out.token.is_empty());
    assert_eq!(out.user_id, user);
    assert_eq!(out.memberships.len(), 1);
    assert_eq!(out.memberships[0].org_id, org);
}

#[tokio::test]
async fn login_wrong_password_returns_invalid_credentials() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "bob", "correct").await;
    let org = seed_org(&pool, "bob-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = login(
        &state,
        LoginInput {
            username_or_email: "bob".into(),
            password: "wrong".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LoginError::InvalidCredentials));
}

#[tokio::test]
async fn login_unknown_user_returns_invalid_credentials() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = login(
        &state,
        LoginInput {
            username_or_email: "nobody".into(),
            password: "x".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LoginError::InvalidCredentials));
}
```

- [ ] Create `tests/security_service_auth_flows_test.rs` (logout + change_password + switch_org):

```rust
#[path = "common/mod.rs"]
mod common;

use egras::security::service::change_password::{change_password, ChangePasswordError, ChangePasswordInput};
use egras::security::service::logout::logout;
use egras::security::service::switch_org::{switch_org, SwitchOrgError, SwitchOrgInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::{grant_role, seed_org, seed_user_with_password};
use uuid::Uuid;

#[tokio::test]
async fn logout_emits_audit_event() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let user = Uuid::now_v7();
    let org = Uuid::now_v7();
    let jti = Uuid::now_v7();

    logout(&state, user, org, jti).await.unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE event_type = 'logout'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn change_password_wrong_current_is_error() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "carol", "original").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = change_password(
        &state,
        user,
        ChangePasswordInput {
            current_password: "wrong".into(),
            new_password: "newpassword1".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ChangePasswordError::WrongCurrentPassword));
}

#[tokio::test]
async fn change_password_happy_path_updates_hash() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "dave", "original").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    change_password(
        &state,
        user,
        ChangePasswordInput {
            current_password: "original".into(),
            new_password: "newpassword1".into(),
        },
    )
    .await
    .unwrap();

    // Verify old hash no longer works by checking the stored hash changed.
    let updated = state.users.find_by_id(user).await.unwrap().unwrap();
    assert!(
        egras::security::service::password::verify_password("newpassword1", &updated.password_hash)
            .unwrap()
    );
}

#[tokio::test]
async fn switch_org_not_member_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "eve", "pass1234").await;
    let home_org = seed_org(&pool, "eve-home", "retail").await;
    let other_org = seed_org(&pool, "other-org", "media").await;
    grant_role(&pool, user, home_org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = switch_org(
        &state,
        user,
        home_org,
        SwitchOrgInput { target_org_id: other_org },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SwitchOrgError::NotMember));
}

#[tokio::test]
async fn switch_org_happy_path_returns_new_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "frank", "pass1234").await;
    let org1 = seed_org(&pool, "frank-org1", "retail").await;
    let org2 = seed_org(&pool, "frank-org2", "media").await;
    grant_role(&pool, user, org1, "org_member").await;
    grant_role(&pool, user, org2, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = switch_org(
        &state,
        user,
        org1,
        SwitchOrgInput { target_org_id: org2 },
    )
    .await
    .unwrap();

    assert!(!out.token.is_empty());
}
```

- [ ] Create `tests/security_service_password_reset_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::security::service::password_reset_confirm::{
    password_reset_confirm, PasswordResetConfirmError, PasswordResetConfirmInput,
};
use egras::security::service::password_reset_request::{
    password_reset_request, PasswordResetRequestInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::seed_user_with_password;

#[tokio::test]
async fn password_reset_unknown_email_returns_ok() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // Must always succeed for unknown email (no information leakage).
    password_reset_request(
        &state,
        PasswordResetRequestInput {
            email: "nobody@example.com".into(),
            base_url: "http://localhost:3000".into(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn password_reset_invalid_token_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = password_reset_confirm(
        &state,
        PasswordResetConfirmInput {
            raw_token: hex::encode([0u8; 32]),
            new_password: "newpass123".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PasswordResetConfirmError::InvalidToken));
}
```

- [ ] Create `tests/tenants_service_add_remove_user_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::tenants::service::add_user_to_organisation::{
    add_user_to_organisation, AddUserToOrganisationInput,
};
use egras::tenants::service::remove_user_from_organisation::{
    remove_user_from_organisation, RemoveUserFromOrganisationError, RemoveUserFromOrganisationInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};
use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn add_user_to_organisation_happy_path() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_add").await;
    let target = seed_user(&pool, "newbie_add").await;
    let org = seed_org(&pool, "org-add", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    add_user_to_organisation(
        &state,
        actor,
        org,
        AddUserToOrganisationInput {
            user_id: target,
            org_id: org,
            role_code: "org_member".into(),
        },
    )
    .await
    .expect("add user");

    let is_member = state.organisations.is_member(target, org).await.unwrap();
    assert!(is_member);
}

#[tokio::test]
async fn remove_last_owner_returns_last_owner_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_rem").await;
    let owner = seed_user(&pool, "owner_rem").await;
    let org = seed_org(&pool, "org-rem", "retail").await;
    grant_role(&pool, owner, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = remove_user_from_organisation(
        &state,
        actor,
        org,
        RemoveUserFromOrganisationInput {
            user_id: owner,
            org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        RemoveUserFromOrganisationError::LastOwner
    ));
}

#[tokio::test]
async fn remove_non_owner_member_succeeds() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_rem2").await;
    let owner = seed_user(&pool, "owner_rem2").await;
    let member = seed_user(&pool, "member_rem2").await;
    let org = seed_org(&pool, "org-rem2", "retail").await;
    grant_role(&pool, owner, org, "org_owner").await;
    grant_role(&pool, member, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    remove_user_from_organisation(
        &state,
        actor,
        org,
        RemoveUserFromOrganisationInput {
            user_id: member,
            org_id: org,
        },
    )
    .await
    .expect("remove member");

    let is_member = state.organisations.is_member(member, org).await.unwrap();
    assert!(!is_member);
}
```

### 10d — HTTP tests

- [ ] Create `tests/security_http_login_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::seed::{grant_role, seed_org, seed_user_with_password};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn login_happy_path_returns_200_with_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "logtest", "hunter2").await;
    let org = seed_org(&pool, "logtest-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({ "username_or_email": "logtest", "password": "hunter2" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert!(!body["token"].as_str().unwrap().is_empty());
    assert_eq!(body["memberships"].as_array().unwrap().len(), 1);

    app.stop().await;
}

#[tokio::test]
async fn login_wrong_password_returns_401() {
    let pool = TestPool::fresh().await.pool;
    seed_user_with_password(&pool, "logtest2", "correct").await;

    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({ "username_or_email": "logtest2", "password": "wrong" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}
```

- [ ] Create `tests/security_http_register_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn register_unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "reg-org1", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .json(&json!({
            "username": "x", "email": "x@x.com",
            "password": "pass1234", "org_id": org, "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn register_without_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "reg_caller").await;
    let org = seed_org(&pool, "reg-org2", "retail").await;
    grant_role(&pool, caller, org, "org_member").await; // no tenants.members.add

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "username": "x", "email": "x@x.com",
            "password": "pass1234", "org_id": org, "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn register_happy_path_returns_201() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "reg_admin").await;
    let org = seed_org(&pool, "reg-org3", "retail").await;
    grant_role(&pool, caller, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool.clone(), cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "username": "newmember",
            "email": "newmember@example.com",
            "password": "securepass1",
            "org_id": org,
            "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["user_id"].is_string());

    app.stop().await;
}
```

- [ ] Create `tests/security_http_auth_flows_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user_with_password};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn logout_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "logout_test", "pass1234").await;
    let org = seed_org(&pool, "logout-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn change_password_wrong_current_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "chpw_test", "original").await;
    let org = seed_org(&pool, "chpw-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/change-password", app.base_url))
        .header("authorization", token)
        .json(&json!({ "current_password": "wrong", "new_password": "newpass12" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn switch_org_not_member_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "sworg_test", "pass1234").await;
    let org = seed_org(&pool, "sworg-home", "retail").await;
    let other = seed_org(&pool, "sworg-other", "media").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/switch-org", app.base_url))
        .header("authorization", token)
        .json(&json!({ "org_id": other }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}
```

- [ ] Create `tests/security_http_password_reset_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn password_reset_request_always_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/password-reset-request", app.base_url))
        .json(&json!({ "email": "nobody@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn password_reset_confirm_invalid_token_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/password-reset-confirm", app.base_url))
        .json(&json!({ "token": hex::encode([0u8; 32]), "new_password": "newpass123" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}
```

- [ ] Create `tests/tenants_http_add_remove_user_test.rs`:

```rust
#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn add_user_to_organisation_happy_path_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "add_caller").await;
    let target = seed_user(&pool, "add_target").await;
    let org = seed_org(&pool, "add-org", "retail").await;
    grant_role(&pool, caller, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/add-user-to-organisation", app.base_url))
        .header("authorization", token)
        .json(&json!({ "user_id": target, "org_id": org, "role_code": "org_member" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn remove_last_owner_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let owner = seed_user(&pool, "rem_owner").await;
    let org = seed_org(&pool, "rem-org", "retail").await;
    grant_role(&pool, owner, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, owner, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/remove-user-from-organisation", app.base_url))
        .header("authorization", token)
        .json(&json!({ "user_id": owner, "org_id": org }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/resource.conflict");
    app.stop().await;
}
```

- [ ] Run full test suite:

```bash
cargo test 2>&1 | tail -40
```

- [ ] Commit:

```bash
git add tests/
git commit -m "test(security): add persistence, service, and HTTP tests for all Plan 2b use cases"
```

---

## Task 11: Final verification + branch summary

- [ ] Full clean build:

```bash
cargo build --release 2>&1 | tail -20
```

- [ ] Full test run:

```bash
cargo test 2>&1 | tail -60
```

- [ ] Clippy clean:

```bash
cargo clippy -- -D warnings 2>&1 | head -60
```

- [ ] Confirm all 9 new endpoints appear in the OpenAPI spec:

```bash
cargo run --bin egras &
sleep 2
curl -s http://localhost:3000/api-docs/openapi.json | python3 -m json.tool | grep '"path"' | head -20
kill %1
```

- [ ] Final commit if any clippy fixes were needed:

```bash
git add -p
git commit -m "chore(security): clippy and formatting fixes"
```
