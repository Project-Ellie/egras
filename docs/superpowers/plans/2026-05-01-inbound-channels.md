# InboundChannel CRUD — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full CRUD for `InboundChannel` — org-scoped reference data that pairs a signal-source label with a server-generated API key — accessible to `operator_admin`, `org_owner`, and `org_admin`.

**Architecture:** New entity under the `tenants/` domain. Five service use-case files, a new repository trait + PG impl, five Axum handlers appended to `tenants/interface.rs`, one new migration (table + permission seed), and a `ChannelsManage` permission extractor. Audit events on write operations; reads are not audited.

**Tech Stack:** Rust, Axum 0.7, sqlx 0.8 (PG), utoipa 4, rand 0.8 + hex 0.4 (api_key generation), chrono, uuid v7.

---

## File Map

**Create:**
- `migrations/0008_inbound_channels.sql`
- `src/tenants/persistence/channel_repository.rs`
- `src/tenants/persistence/channel_repository_pg.rs`
- `src/tenants/service/create_inbound_channel.rs`
- `src/tenants/service/list_inbound_channels.rs`
- `src/tenants/service/get_inbound_channel.rs`
- `src/tenants/service/update_inbound_channel.rs`
- `src/tenants/service/delete_inbound_channel.rs`
- `tests/tenants_persistence_channels_test.rs`
- `tests/tenants_service_channels_test.rs`
- `tests/tenants_http_channels_test.rs`

**Modify:**
- `src/tenants/model.rs` — add `ChannelType`, `InboundChannel`, `ChannelCursor`
- `src/tenants/persistence/mod.rs` — re-export channel repository
- `src/tenants/service/mod.rs` — declare new service modules
- `src/tenants/interface.rs` — 5 new handlers + route registrations
- `src/auth/extractors.rs` — add `ChannelsManage` permission marker
- `src/audit/model.rs` — add 3 audit event factory methods
- `src/app_state.rs` — add `inbound_channels` field
- `src/testing.rs` — extend `MockAppStateBuilder` with channels repo
- `src/lib.rs` — wire `InboundChannelRepositoryPg` into `AppState`
- `src/openapi.rs` — register new paths and schemas
- `docs/openapi.json` — regenerate

---

## Task 1: Migration

**Files:**
- Create: `migrations/0008_inbound_channels.sql`

- [ ] **Step 1: Write the migration**

```sql
-- migrations/0008_inbound_channels.sql

CREATE TABLE inbound_channels (
    id               UUID        PRIMARY KEY,
    organisation_id  UUID        NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    name             TEXT        NOT NULL,
    description      TEXT,
    channel_type     TEXT        NOT NULL CHECK (channel_type IN ('vast', 'sensor', 'websocket', 'rest')),
    api_key          TEXT        NOT NULL,
    is_active        BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX inbound_channels_organisation_id_name_key
    ON inbound_channels (organisation_id, name);

-- New permission
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-00000000020c', 'channels.manage',
   'Manage inbound channels for an organisation')
ON CONFLICT (id) DO NOTHING;

-- operator_admin, org_owner, org_admin get channels.manage
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.code IN ('operator_admin', 'org_owner', 'org_admin')
  AND p.code = 'channels.manage'
ON CONFLICT DO NOTHING;
```

- [ ] **Step 2: Apply the migration to verify it parses**

```bash
docker-compose up -d postgres
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo sqlx migrate run --database-url postgres://postgres:secret@localhost:5432/egras_test
```

Expected: migration runs without error.

- [ ] **Step 3: Commit**

```bash
git add migrations/0008_inbound_channels.sql
git commit -m "feat(channels): migration — inbound_channels table + channels.manage permission"
```

---

## Task 2: Model Types

**Files:**
- Modify: `src/tenants/model.rs`

- [ ] **Step 1: Add `ChannelType`, `InboundChannel`, `ChannelCursor` to the model**

Append to `src/tenants/model.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Vast,
    Sensor,
    Websocket,
    Rest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundChannel {
    pub id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub api_key: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo check
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/tenants/model.rs
git commit -m "feat(channels): add ChannelType, InboundChannel, ChannelCursor model types"
```

---

## Task 3: Repository Trait

**Files:**
- Create: `src/tenants/persistence/channel_repository.rs`

- [ ] **Step 1: Write the trait and error type**

```rust
// src/tenants/persistence/channel_repository.rs

use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::{ChannelCursor, ChannelType, InboundChannel};

#[derive(Debug, thiserror::Error)]
pub enum ChannelRepoError {
    #[error("duplicate channel name: {0}")]
    DuplicateName(String),
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait InboundChannelRepository: Send + Sync + 'static {
    /// Insert a new channel. Generates id and api_key internally.
    async fn create(
        &self,
        organisation_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError>;

    async fn list(
        &self,
        organisation_id: Uuid,
        after: Option<ChannelCursor>,
        limit: u32,
    ) -> Result<Vec<InboundChannel>, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn get(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<InboundChannel, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn update(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn delete(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<(), ChannelRepoError>;
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo check
```

Expected: no errors (trait is defined but not yet registered in mod.rs).

- [ ] **Step 3: Commit**

```bash
git add src/tenants/persistence/channel_repository.rs
git commit -m "feat(channels): InboundChannelRepository trait + ChannelRepoError"
```

---

## Task 4: Repository PG Implementation + Persistence Tests

**Files:**
- Create: `src/tenants/persistence/channel_repository_pg.rs`
- Modify: `src/tenants/persistence/mod.rs`
- Create: `tests/tenants_persistence_channels_test.rs`

- [ ] **Step 1: Write the failing persistence tests**

```rust
// tests/tenants_persistence_channels_test.rs

use egras::tenants::model::ChannelType;
use egras::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};
use egras::tenants::persistence::channel_repository_pg::InboundChannelRepositoryPg;
use egras::testing::TestPool;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_org(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO organisations (id, name, business, is_operator) VALUES ($1, $2, 'test', FALSE)",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await
    .expect("seed org");
    id
}

#[tokio::test]
async fn create_returns_channel_with_generated_api_key() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo
        .create(org, "VAST Feed", Some("main VAST feed"), ChannelType::Vast, true)
        .await
        .unwrap();

    assert_eq!(ch.organisation_id, org);
    assert_eq!(ch.name, "VAST Feed");
    assert_eq!(ch.description, Some("main VAST feed".into()));
    assert_eq!(ch.channel_type, ChannelType::Vast);
    assert!(ch.is_active);
    assert_eq!(ch.api_key.len(), 64);
}

#[tokio::test]
async fn create_duplicate_name_in_same_org_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme2").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.create(org, "feed", None, ChannelType::Sensor, true).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::DuplicateName(n) if n == "feed"));
}

#[tokio::test]
async fn duplicate_name_in_different_org_is_allowed() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-a").await;
    let org2 = seed_org(&pool, "org-b").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    repo.create(org2, "feed", None, ChannelType::Rest, true).await.unwrap(); // must not fail
}

#[tokio::test]
async fn get_returns_not_found_for_wrong_org() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-c").await;
    let org2 = seed_org(&pool, "org-d").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.get(org2, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}

#[tokio::test]
async fn list_returns_channels_for_org_only() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-e").await;
    let org2 = seed_org(&pool, "org-f").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org1, "feed-a", None, ChannelType::Vast, true).await.unwrap();
    repo.create(org1, "feed-b", None, ChannelType::Sensor, true).await.unwrap();
    repo.create(org2, "feed-x", None, ChannelType::Rest, true).await.unwrap();

    let items = repo.list(org1, None, 50).await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|c| c.organisation_id == org1));
}

#[tokio::test]
async fn update_changes_mutable_fields_and_bumps_updated_at() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "org-g").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org, "old-name", None, ChannelType::Vast, true).await.unwrap();
    let original_key = ch.api_key.clone();

    let updated = repo
        .update(org, ch.id, "new-name", Some("desc"), ChannelType::Sensor, false)
        .await
        .unwrap();

    assert_eq!(updated.name, "new-name");
    assert_eq!(updated.description, Some("desc".into()));
    assert_eq!(updated.channel_type, ChannelType::Sensor);
    assert!(!updated.is_active);
    assert_eq!(updated.api_key, original_key); // key must not change
}

#[tokio::test]
async fn delete_removes_channel_and_get_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "org-h").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org, "feed", None, ChannelType::Rest, true).await.unwrap();
    repo.delete(org, ch.id).await.unwrap();
    let err = repo.get(org, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}

#[tokio::test]
async fn delete_wrong_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-i").await;
    let org2 = seed_org(&pool, "org-j").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.delete(org2, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features tenants_persistence_channels 2>&1 | tail -20
```

Expected: compilation error — `InboundChannelRepositoryPg` not yet defined.

- [ ] **Step 3: Write the PG implementation**

```rust
// src/tenants/persistence/channel_repository_pg.rs

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use sqlx::PgPool;
use uuid::Uuid;

use super::channel_repository::{ChannelRepoError, InboundChannelRepository};
use crate::tenants::model::{ChannelCursor, ChannelType, InboundChannel};

pub struct InboundChannelRepositoryPg {
    pool: PgPool,
}

impl InboundChannelRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn generate_api_key() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

fn map_insert_error(err: sqlx::Error, name: &str) -> ChannelRepoError {
    if let sqlx::Error::Database(ref dbe) = err {
        if dbe.code().as_deref() == Some("23505")
            && dbe
                .constraint()
                .map(|c| c.contains("organisation_id_name"))
                .unwrap_or(false)
        {
            return ChannelRepoError::DuplicateName(name.to_string());
        }
    }
    ChannelRepoError::Db(err)
}

#[async_trait]
impl InboundChannelRepository for InboundChannelRepositoryPg {
    async fn create(
        &self,
        organisation_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let id = Uuid::now_v7();
        let api_key = generate_api_key();
        let row = sqlx::query_as::<_, ChannelRow>(
            "INSERT INTO inbound_channels \
             (id, organisation_id, name, description, channel_type, api_key, is_active) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, organisation_id, name, description, channel_type, api_key, \
                       is_active, created_at, updated_at",
        )
        .bind(id)
        .bind(organisation_id)
        .bind(name)
        .bind(description)
        .bind(&channel_type)
        .bind(&api_key)
        .bind(is_active)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| map_insert_error(e, name))?;

        Ok(row.into())
    }

    async fn list(
        &self,
        organisation_id: Uuid,
        after: Option<ChannelCursor>,
        limit: u32,
    ) -> Result<Vec<InboundChannel>, ChannelRepoError> {
        let rows: Vec<ChannelRow> = if let Some(cursor) = after {
            sqlx::query_as::<_, ChannelRow>(
                "SELECT id, organisation_id, name, description, channel_type, api_key, \
                        is_active, created_at, updated_at \
                 FROM inbound_channels \
                 WHERE organisation_id = $1 \
                   AND (created_at, id) < ($2, $3) \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $4",
            )
            .bind(organisation_id)
            .bind(cursor.created_at)
            .bind(cursor.id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, ChannelRow>(
                "SELECT id, organisation_id, name, description, channel_type, api_key, \
                        is_active, created_at, updated_at \
                 FROM inbound_channels \
                 WHERE organisation_id = $1 \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $2",
            )
            .bind(organisation_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(InboundChannel::from).collect())
    }

    async fn get(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let row = sqlx::query_as::<_, ChannelRow>(
            "SELECT id, organisation_id, name, description, channel_type, api_key, \
                    is_active, created_at, updated_at \
             FROM inbound_channels \
             WHERE id = $1 AND organisation_id = $2",
        )
        .bind(channel_id)
        .bind(organisation_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ChannelRepoError::NotFound)?;

        Ok(row.into())
    }

    async fn update(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let row = sqlx::query_as::<_, ChannelRow>(
            "UPDATE inbound_channels \
             SET name = $1, description = $2, channel_type = $3, is_active = $4, \
                 updated_at = now() \
             WHERE id = $5 AND organisation_id = $6 \
             RETURNING id, organisation_id, name, description, channel_type, api_key, \
                       is_active, created_at, updated_at",
        )
        .bind(name)
        .bind(description)
        .bind(&channel_type)
        .bind(is_active)
        .bind(channel_id)
        .bind(organisation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| map_insert_error(e, name))?
        .ok_or(ChannelRepoError::NotFound)?;

        Ok(row.into())
    }

    async fn delete(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<(), ChannelRepoError> {
        let result = sqlx::query(
            "DELETE FROM inbound_channels WHERE id = $1 AND organisation_id = $2",
        )
        .bind(channel_id)
        .bind(organisation_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ChannelRepoError::NotFound);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal row struct
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ChannelRow {
    id: Uuid,
    organisation_id: Uuid,
    name: String,
    description: Option<String>,
    channel_type: ChannelType,
    api_key: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ChannelRow> for InboundChannel {
    fn from(r: ChannelRow) -> Self {
        InboundChannel {
            id: r.id,
            organisation_id: r.organisation_id,
            name: r.name,
            description: r.description,
            channel_type: r.channel_type,
            api_key: r.api_key,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
```

- [ ] **Step 4: Register in `src/tenants/persistence/mod.rs`**

Append to the existing content:

```rust
pub mod channel_repository;
pub mod channel_repository_pg;

pub use channel_repository::{ChannelRepoError, InboundChannelRepository};
pub use channel_repository_pg::InboundChannelRepositoryPg;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features tenants_persistence_channels 2>&1 | tail -20
```

Expected: all 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/tenants/persistence/channel_repository.rs \
        src/tenants/persistence/channel_repository_pg.rs \
        src/tenants/persistence/mod.rs \
        tests/tenants_persistence_channels_test.rs
git commit -m "feat(channels): InboundChannelRepositoryPg + persistence tests"
```

---

## Task 5: Wire Repository into AppState

**Files:**
- Modify: `src/app_state.rs`
- Modify: `src/testing.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add `inbound_channels` field to `AppState`**

In `src/app_state.rs`, add the import and field:

```rust
// Add to existing imports:
use crate::tenants::persistence::InboundChannelRepository;

// Add field to AppState struct:
pub inbound_channels: Arc<dyn InboundChannelRepository>,
```

- [ ] **Step 2: Extend `MockAppStateBuilder` in `src/testing.rs`**

Add the field to `MockAppStateBuilder`:

```rust
// In the struct:
inbound_channels: Option<Arc<dyn crate::tenants::persistence::InboundChannelRepository>>,
```

In `MockAppStateBuilder::new`, add:
```rust
inbound_channels: None,
```

Add a builder method:
```rust
pub fn with_pg_channels_repo(mut self) -> Self {
    self.inbound_channels = Some(Arc::new(
        crate::tenants::persistence::InboundChannelRepositoryPg::new(self.pool.clone()),
    ));
    self
}

pub fn inbound_channels(
    mut self,
    r: Arc<dyn crate::tenants::persistence::InboundChannelRepository>,
) -> Self {
    self.inbound_channels = Some(r);
    self
}
```

In `build()`, add:
```rust
inbound_channels: self.inbound_channels.expect("inbound_channels not set"),
```

- [ ] **Step 3: Wire in `src/lib.rs`**

After the existing `roles` construction in `build_app`, add:

```rust
let inbound_channels: Arc<dyn crate::tenants::persistence::InboundChannelRepository> = Arc::new(
    crate::tenants::persistence::InboundChannelRepositoryPg::new(pool.clone()),
);
```

Add to the `AppState { ... }` literal:
```rust
inbound_channels,
```

- [ ] **Step 4: Update all `MockAppStateBuilder::build()` call sites**

Every existing test that calls `MockAppStateBuilder::new(...).build()` must now also call `.with_pg_channels_repo()` (or `.inbound_channels(...)` if using a mock). Search for all call sites:

```bash
grep -rn "MockAppStateBuilder" tests/
```

Add `.with_pg_channels_repo()` before `.build()` in each call chain that uses `.with_pg_tenants_repos()`.

- [ ] **Step 5: Verify full compile and existing tests pass**

```bash
cargo check && \
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features 2>&1 | tail -30
```

Expected: compiles cleanly, all existing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/app_state.rs src/testing.rs src/lib.rs
git commit -m "feat(channels): wire InboundChannelRepository into AppState"
```

---

## Task 6: Permission Extractor

**Files:**
- Modify: `src/auth/extractors.rs`

- [ ] **Step 1: Add `ChannelsManage` marker**

Append to `src/auth/extractors.rs`:

```rust
/// Permission marker: `channels.manage`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct ChannelsManage;
impl Permission for ChannelsManage {
    const CODE: &'static str = "channels.manage";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}
```

- [ ] **Step 2: Verify compile**

```bash
cargo check
```

- [ ] **Step 3: Commit**

```bash
git add src/auth/extractors.rs
git commit -m "feat(channels): ChannelsManage permission extractor"
```

---

## Task 7: Audit Event Factory Methods

**Files:**
- Modify: `src/audit/model.rs`

- [ ] **Step 1: Add three factory methods to `AuditEvent`**

Find the `impl AuditEvent` block in `src/audit/model.rs` and append:

```rust
pub fn channel_created(
    actor: Uuid,
    actor_org: Uuid,
    channel_id: Uuid,
    org_id: Uuid,
    name: &str,
) -> Self {
    let mut e = Self::base(
        AuditCategory::TenantsStateChange,
        "channel.created",
        Outcome::Success,
    );
    e.actor_user_id = Some(actor);
    e.actor_organisation_id = Some(actor_org);
    e.target_type = Some("inbound_channel".into());
    e.target_id = Some(channel_id);
    e.target_organisation_id = Some(org_id);
    e.metadata = Some(serde_json::json!({ "name": name }));
    e
}

pub fn channel_updated(
    actor: Uuid,
    actor_org: Uuid,
    channel_id: Uuid,
    org_id: Uuid,
    name: &str,
) -> Self {
    let mut e = Self::base(
        AuditCategory::TenantsStateChange,
        "channel.updated",
        Outcome::Success,
    );
    e.actor_user_id = Some(actor);
    e.actor_organisation_id = Some(actor_org);
    e.target_type = Some("inbound_channel".into());
    e.target_id = Some(channel_id);
    e.target_organisation_id = Some(org_id);
    e.metadata = Some(serde_json::json!({ "name": name }));
    e
}

pub fn channel_deleted(
    actor: Uuid,
    actor_org: Uuid,
    channel_id: Uuid,
    org_id: Uuid,
) -> Self {
    let mut e = Self::base(
        AuditCategory::TenantsStateChange,
        "channel.deleted",
        Outcome::Success,
    );
    e.actor_user_id = Some(actor);
    e.actor_organisation_id = Some(actor_org);
    e.target_type = Some("inbound_channel".into());
    e.target_id = Some(channel_id);
    e.target_organisation_id = Some(org_id);
    e
}
```

- [ ] **Step 2: Verify compile**

```bash
cargo check
```

- [ ] **Step 3: Commit**

```bash
git add src/audit/model.rs
git commit -m "feat(channels): audit event factory methods for channel.created/updated/deleted"
```

---

## Task 8: Service — create_inbound_channel

**Files:**
- Create: `src/tenants/service/create_inbound_channel.rs`
- Modify: `src/tenants/service/mod.rs`

- [ ] **Step 1: Write the service file**

```rust
// src/tenants/service/create_inbound_channel.rs

use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::model::{ChannelType, InboundChannel};
use crate::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};

#[derive(Debug, Clone)]
pub struct CreateChannelInput {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateChannelError {
    #[error("channel name already taken")]
    DuplicateName,
    #[error("invalid name")]
    InvalidName,
    #[error("invalid description")]
    InvalidDescription,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn create_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    input: CreateChannelInput,
) -> Result<InboundChannel, CreateChannelError> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(CreateChannelError::InvalidName);
    }
    if let Some(ref d) = input.description {
        if d.chars().count() > 1000 {
            return Err(CreateChannelError::InvalidDescription);
        }
    }

    let ch = match state
        .inbound_channels
        .create(
            input.organisation_id,
            &name,
            input.description.as_deref(),
            input.channel_type,
            input.is_active,
        )
        .await
    {
        Ok(ch) => ch,
        Err(ChannelRepoError::DuplicateName(_)) => return Err(CreateChannelError::DuplicateName),
        Err(e) => return Err(CreateChannelError::Repo(e)),
    };

    let event = AuditEvent::channel_created(actor_user_id, actor_org, ch.id, ch.organisation_id, &ch.name);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %ch.id, "audit record failed for channel.created");
    }

    Ok(ch)
}
```

- [ ] **Step 2: Add `pub mod create_inbound_channel;` to `src/tenants/service/mod.rs`**

- [ ] **Step 3: Verify compile**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add src/tenants/service/create_inbound_channel.rs src/tenants/service/mod.rs
git commit -m "feat(channels): create_inbound_channel service"
```

---

## Task 9: Service — list_inbound_channels

**Files:**
- Create: `src/tenants/service/list_inbound_channels.rs`
- Modify: `src/tenants/service/mod.rs`

- [ ] **Step 1: Write the service file**

```rust
// src/tenants/service/list_inbound_channels.rs

use uuid::Uuid;

use crate::app_state::AppState;
use crate::pagination as cursor_codec;
use crate::tenants::model::{ChannelCursor, ChannelType, InboundChannel};
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, Clone)]
pub struct ListChannelsInput {
    pub organisation_id: Uuid,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListChannelsOutput {
    pub items: Vec<InboundChannel>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListChannelsError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn list_inbound_channels(
    state: &AppState,
    input: ListChannelsInput,
) -> Result<ListChannelsOutput, ListChannelsError> {
    let after: Option<ChannelCursor> = match input.after {
        Some(ref raw) => Some(
            cursor_codec::decode::<ChannelCursor>(raw)
                .map_err(|_| ListChannelsError::InvalidCursor)?,
        ),
        None => None,
    };

    let limit = input.limit;
    let mut items = state
        .inbound_channels
        .list(input.organisation_id, after, limit + 1)
        .await?;

    let next_cursor = if items.len() as u32 > limit {
        items.truncate(limit as usize);
        items.last().map(|ch| {
            cursor_codec::encode(&ChannelCursor {
                created_at: ch.created_at,
                id: ch.id,
            })
        })
    } else {
        None
    };

    Ok(ListChannelsOutput { items, next_cursor })
}
```

- [ ] **Step 2: Add `pub mod list_inbound_channels;` to `src/tenants/service/mod.rs`**

- [ ] **Step 3: Verify compile**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add src/tenants/service/list_inbound_channels.rs src/tenants/service/mod.rs
git commit -m "feat(channels): list_inbound_channels service"
```

---

## Task 10: Service — get_inbound_channel

**Files:**
- Create: `src/tenants/service/get_inbound_channel.rs`
- Modify: `src/tenants/service/mod.rs`

- [ ] **Step 1: Write the service file**

```rust
// src/tenants/service/get_inbound_channel.rs

use uuid::Uuid;

use crate::app_state::AppState;
use crate::tenants::model::InboundChannel;
use crate::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};

#[derive(Debug, thiserror::Error)]
pub enum GetChannelError {
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn get_inbound_channel(
    state: &AppState,
    organisation_id: Uuid,
    channel_id: Uuid,
) -> Result<InboundChannel, GetChannelError> {
    match state.inbound_channels.get(organisation_id, channel_id).await {
        Ok(ch) => Ok(ch),
        Err(ChannelRepoError::NotFound) => Err(GetChannelError::NotFound),
        Err(e) => Err(GetChannelError::Repo(e)),
    }
}
```

- [ ] **Step 2: Add `pub mod get_inbound_channel;` to `src/tenants/service/mod.rs`**

- [ ] **Step 3: Verify compile**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add src/tenants/service/get_inbound_channel.rs src/tenants/service/mod.rs
git commit -m "feat(channels): get_inbound_channel service"
```

---

## Task 11: Service — update_inbound_channel

**Files:**
- Create: `src/tenants/service/update_inbound_channel.rs`
- Modify: `src/tenants/service/mod.rs`

- [ ] **Step 1: Write the service file**

```rust
// src/tenants/service/update_inbound_channel.rs

use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::model::{ChannelType, InboundChannel};
use crate::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};

#[derive(Debug, Clone)]
pub struct UpdateChannelInput {
    pub organisation_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateChannelError {
    #[error("channel not found")]
    NotFound,
    #[error("channel name already taken")]
    DuplicateName,
    #[error("invalid name")]
    InvalidName,
    #[error("invalid description")]
    InvalidDescription,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn update_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    input: UpdateChannelInput,
) -> Result<InboundChannel, UpdateChannelError> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(UpdateChannelError::InvalidName);
    }
    if let Some(ref d) = input.description {
        if d.chars().count() > 1000 {
            return Err(UpdateChannelError::InvalidDescription);
        }
    }

    let ch = match state
        .inbound_channels
        .update(
            input.organisation_id,
            input.channel_id,
            &name,
            input.description.as_deref(),
            input.channel_type,
            input.is_active,
        )
        .await
    {
        Ok(ch) => ch,
        Err(ChannelRepoError::NotFound) => return Err(UpdateChannelError::NotFound),
        Err(ChannelRepoError::DuplicateName(_)) => return Err(UpdateChannelError::DuplicateName),
        Err(e) => return Err(UpdateChannelError::Repo(e)),
    };

    let event = AuditEvent::channel_updated(actor_user_id, actor_org, ch.id, ch.organisation_id, &ch.name);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %ch.id, "audit record failed for channel.updated");
    }

    Ok(ch)
}
```

- [ ] **Step 2: Add `pub mod update_inbound_channel;` to `src/tenants/service/mod.rs`**

- [ ] **Step 3: Verify compile**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add src/tenants/service/update_inbound_channel.rs src/tenants/service/mod.rs
git commit -m "feat(channels): update_inbound_channel service"
```

---

## Task 12: Service — delete_inbound_channel

**Files:**
- Create: `src/tenants/service/delete_inbound_channel.rs`
- Modify: `src/tenants/service/mod.rs`

- [ ] **Step 1: Write the service file**

```rust
// src/tenants/service/delete_inbound_channel.rs

use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};

#[derive(Debug, thiserror::Error)]
pub enum DeleteChannelError {
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn delete_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    organisation_id: Uuid,
    channel_id: Uuid,
) -> Result<(), DeleteChannelError> {
    match state
        .inbound_channels
        .delete(organisation_id, channel_id)
        .await
    {
        Ok(()) => {}
        Err(ChannelRepoError::NotFound) => return Err(DeleteChannelError::NotFound),
        Err(e) => return Err(DeleteChannelError::Repo(e)),
    }

    let event = AuditEvent::channel_deleted(actor_user_id, actor_org, channel_id, organisation_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %channel_id, "audit record failed for channel.deleted");
    }

    Ok(())
}
```

- [ ] **Step 2: Add `pub mod delete_inbound_channel;` to `src/tenants/service/mod.rs`**

- [ ] **Step 3: Write service-level tests (all five operations)**

```rust
// tests/tenants_service_channels_test.rs

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use egras::audit::persistence::{AuditRepository, AuditRepositoryPg};
use egras::audit::service::ListAuditEventsImpl;
use egras::tenants::model::ChannelType;
use egras::tenants::service::create_inbound_channel::{
    create_inbound_channel, CreateChannelError, CreateChannelInput,
};
use egras::tenants::service::delete_inbound_channel::delete_inbound_channel;
use egras::tenants::service::get_inbound_channel::{get_inbound_channel, GetChannelError};
use egras::tenants::service::list_inbound_channels::{list_inbound_channels, ListChannelsInput};
use egras::tenants::service::update_inbound_channel::{
    update_inbound_channel, UpdateChannelError, UpdateChannelInput,
};
use egras::testing::{BlockingAuditRecorder, MockAppStateBuilder, TestPool};

use common::seed::{seed_org, seed_user};

#[tokio::test]
async fn create_happy_path_returns_channel_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;

    let audit_repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(audit_repo.clone()));
    let state = MockAppStateBuilder::new(pool)
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(ListAuditEventsImpl::new(audit_repo)))
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "VAST Feed".into(),
            description: Some("main feed".into()),
            channel_type: ChannelType::Vast,
            is_active: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(ch.name, "VAST Feed");
    assert_eq!(ch.api_key.len(), 64);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.created");
}

#[tokio::test]
async fn create_duplicate_name_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice2").await;
    let org = seed_org(&pool, "acme2", "retail").await;
    let state = MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    let input = CreateChannelInput {
        organisation_id: org,
        name: "feed".into(),
        description: None,
        channel_type: ChannelType::Rest,
        is_active: true,
    };
    create_inbound_channel(&state, actor, org, input.clone()).await.unwrap();
    let err = create_inbound_channel(&state, actor, org, input).await.unwrap_err();
    assert!(matches!(err, CreateChannelError::DuplicateName));
}

#[tokio::test]
async fn get_wrong_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice3").await;
    let org1 = seed_org(&pool, "org-k", "retail").await;
    let org2 = seed_org(&pool, "org-l", "retail").await;
    let state = MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    let ch = create_inbound_channel(
        &state,
        actor,
        org1,
        CreateChannelInput {
            organisation_id: org1,
            name: "feed".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let err = get_inbound_channel(&state, org2, ch.id).await.unwrap_err();
    assert!(matches!(err, GetChannelError::NotFound));
}

#[tokio::test]
async fn update_changes_fields_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice4").await;
    let org = seed_org(&pool, "org-m", "retail").await;

    let audit_repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(audit_repo.clone()));
    let state = MockAppStateBuilder::new(pool)
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(ListAuditEventsImpl::new(audit_repo)))
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "old".into(),
            description: None,
            channel_type: ChannelType::Vast,
            is_active: true,
        },
    )
    .await
    .unwrap();

    recorder.captured.lock().await.clear();

    let updated = update_inbound_channel(
        &state,
        actor,
        org,
        UpdateChannelInput {
            organisation_id: org,
            channel_id: ch.id,
            name: "new".into(),
            description: Some("desc".into()),
            channel_type: ChannelType::Sensor,
            is_active: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.name, "new");
    assert_eq!(updated.api_key, ch.api_key);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.updated");
}

#[tokio::test]
async fn delete_emits_audit_and_get_returns_not_found_after() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice5").await;
    let org = seed_org(&pool, "org-n", "retail").await;

    let audit_repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(audit_repo.clone()));
    let state = MockAppStateBuilder::new(pool)
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(ListAuditEventsImpl::new(audit_repo)))
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "to-delete".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    recorder.captured.lock().await.clear();

    delete_inbound_channel(&state, actor, org, org, ch.id).await.unwrap();

    let err = get_inbound_channel(&state, org, ch.id).await.unwrap_err();
    assert!(matches!(err, GetChannelError::NotFound));

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.deleted");
}

#[tokio::test]
async fn update_name_collision_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice6").await;
    let org = seed_org(&pool, "org-o", "retail").await;
    let state = MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();

    create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "taken".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let ch2 = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "other".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let err = update_inbound_channel(
        &state,
        actor,
        org,
        UpdateChannelInput {
            organisation_id: org,
            channel_id: ch2.id,
            name: "taken".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, UpdateChannelError::DuplicateName));
}
```


- [ ] **Step 4: Run service tests**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features tenants_service_channels 2>&1 | tail -20
```

Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tenants/service/delete_inbound_channel.rs \
        src/tenants/service/mod.rs \
        tests/tenants_service_channels_test.rs
git commit -m "feat(channels): delete_inbound_channel service + full service test suite"
```

---

## Task 13: Interface Handlers + HTTP Tests

**Files:**
- Modify: `src/tenants/interface.rs`
- Create: `tests/tenants_http_channels_test.rs`

- [ ] **Step 1: Add DTOs and all five handlers to `src/tenants/interface.rs`**

Add new imports at the top of `interface.rs`:

```rust
use crate::auth::extractors::ChannelsManage;
use crate::tenants::service::create_inbound_channel::{
    create_inbound_channel, CreateChannelError, CreateChannelInput,
};
use crate::tenants::service::delete_inbound_channel::{delete_inbound_channel, DeleteChannelError};
use crate::tenants::service::get_inbound_channel::{get_inbound_channel, GetChannelError};
use crate::tenants::service::list_inbound_channels::{list_inbound_channels, ListChannelsError, ListChannelsInput};
use crate::tenants::service::update_inbound_channel::{
    update_inbound_channel, UpdateChannelError, UpdateChannelInput,
};
use crate::tenants::model::ChannelType;
```

Add routes to `router()`:

```rust
.route(
    "/organisations/:org_id/channels",
    post(post_create_channel).get(get_list_channels),
)
.route(
    "/organisations/:org_id/channels/:channel_id",
    get(get_channel).put(put_update_channel).delete(delete_channel),
)
```

Add request/response DTOs:

```rust
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateChannelRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    pub channel_type: ChannelType,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateChannelRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChannelBody {
    pub id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub api_key: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PagedChannels {
    pub items: Vec<ChannelBody>,
    pub next_cursor: Option<String>,
}

fn channel_body(ch: crate::tenants::model::InboundChannel) -> ChannelBody {
    ChannelBody {
        id: ch.id,
        organisation_id: ch.organisation_id,
        name: ch.name,
        description: ch.description,
        channel_type: ch.channel_type,
        api_key: ch.api_key,
        is_active: ch.is_active,
        created_at: ch.created_at,
        updated_at: ch.updated_at,
    }
}
```

Add handler functions:

```rust
#[utoipa::path(
    post,
    path = "/api/v1/tenants/organisations/{org_id}/channels",
    tag = "tenants",
    request_body = CreateChannelRequest,
    security(("bearer" = [])),
    params(("org_id" = Uuid, Path, description = "Organisation ID")),
    responses(
        (status = 201, description = "Channel created", body = ChannelBody),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation not found", body = ErrorBody),
        (status = 409, description = "Channel name already taken", body = ErrorBody),
    ),
)]
pub async fn post_create_channel(
    _perm: Perm<ChannelsManage>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    Json(req): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelBody>), AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound { resource: "organisation".into() });
    }
    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;
    let ch = create_inbound_channel(
        &state,
        caller.claims.sub,
        caller.claims.org,
        CreateChannelInput {
            organisation_id: org_id,
            name: req.name,
            description: req.description,
            channel_type: req.channel_type,
            is_active: req.is_active,
        },
    )
    .await
    .map_err(|e| match e {
        CreateChannelError::DuplicateName => AppError::Conflict {
            slug: "channel_name_taken".into(),
        },
        CreateChannelError::InvalidName | CreateChannelError::InvalidDescription => {
            AppError::Validation { errors: Default::default() }
        }
        CreateChannelError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    })?;
    Ok((StatusCode::CREATED, Json(channel_body(ch))))
}

#[utoipa::path(
    get,
    path = "/api/v1/tenants/organisations/{org_id}/channels",
    tag = "tenants",
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("after" = Option<String>, Query, description = "Pagination cursor"),
        ("limit" = Option<u32>, Query, description = "Max items (default 50)"),
    ),
    responses(
        (status = 200, description = "List of channels", body = PagedChannels),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation not found", body = ErrorBody),
    ),
)]
pub async fn get_list_channels(
    _perm: Perm<ChannelsManage>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Result<Json<PagedChannels>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound { resource: "organisation".into() });
    }
    let out = list_inbound_channels(
        &state,
        ListChannelsInput {
            organisation_id: org_id,
            after: q.after,
            limit: q.limit.unwrap_or(50),
        },
    )
    .await
    .map_err(|e| match e {
        ListChannelsError::InvalidCursor => AppError::Validation {
            errors: {
                let mut m = std::collections::HashMap::new();
                m.insert("after".into(), vec!["invalid_cursor".into()]);
                m
            },
        },
        ListChannelsError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    })?;
    Ok(Json(PagedChannels {
        items: out.items.into_iter().map(channel_body).collect(),
        next_cursor: out.next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
    tag = "tenants",
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("channel_id" = Uuid, Path, description = "Channel ID"),
    ),
    responses(
        (status = 200, description = "Channel", body = ChannelBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Channel not found", body = ErrorBody),
    ),
)]
pub async fn get_channel(
    _perm: Perm<ChannelsManage>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<Json<ChannelBody>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound { resource: "organisation".into() });
    }
    let ch = get_inbound_channel(&state, org_id, channel_id)
        .await
        .map_err(|e| match e {
            GetChannelError::NotFound => AppError::NotFound {
                resource: "channel".into(),
            },
            GetChannelError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        })?;
    Ok(Json(channel_body(ch)))
}

#[utoipa::path(
    put,
    path = "/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
    tag = "tenants",
    request_body = UpdateChannelRequest,
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("channel_id" = Uuid, Path, description = "Channel ID"),
    ),
    responses(
        (status = 200, description = "Updated channel", body = ChannelBody),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Channel not found", body = ErrorBody),
        (status = 409, description = "Channel name already taken", body = ErrorBody),
    ),
)]
pub async fn put_update_channel(
    _perm: Perm<ChannelsManage>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateChannelRequest>,
) -> Result<Json<ChannelBody>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound { resource: "organisation".into() });
    }
    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;
    let ch = update_inbound_channel(
        &state,
        caller.claims.sub,
        caller.claims.org,
        UpdateChannelInput {
            organisation_id: org_id,
            channel_id,
            name: req.name,
            description: req.description,
            channel_type: req.channel_type,
            is_active: req.is_active,
        },
    )
    .await
    .map_err(|e| match e {
        UpdateChannelError::NotFound => AppError::NotFound {
            resource: "channel".into(),
        },
        UpdateChannelError::DuplicateName => AppError::Conflict {
            slug: "channel_name_taken".into(),
        },
        UpdateChannelError::InvalidName | UpdateChannelError::InvalidDescription => {
            AppError::Validation { errors: Default::default() }
        }
        UpdateChannelError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    })?;
    Ok(Json(channel_body(ch)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
    tag = "tenants",
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("channel_id" = Uuid, Path, description = "Channel ID"),
    ),
    responses(
        (status = 204, description = "Channel deleted"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Channel not found", body = ErrorBody),
    ),
)]
pub async fn delete_channel(
    _perm: Perm<ChannelsManage>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound { resource: "organisation".into() });
    }
    delete_inbound_channel(&state, caller.claims.sub, caller.claims.org, org_id, channel_id)
        .await
        .map_err(|e| match e {
            DeleteChannelError::NotFound => AppError::NotFound {
                resource: "channel".into(),
            },
            DeleteChannelError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        })?;
    Ok(StatusCode::NO_CONTENT)
}
```


- [ ] **Step 2: Verify compile**

```bash
cargo check
```

- [ ] **Step 3: Write HTTP integration tests**

```rust
// tests/tenants_http_channels_test.rs

#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_create_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme-401", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url))
        .json(&json!({ "name": "feed", "channel_type": "vast" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn org_member_cannot_create_channel_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice-403").await;
    let org = seed_org(&pool, "acme-403", "retail").await;
    grant_role(&pool, user, org, "org_member").await;
    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "feed", "channel_type": "vast" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn org_admin_can_create_channel_happy_path() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice-201").await;
    let org = seed_org(&pool, "acme-201", "retail").await;
    grant_role(&pool, user, org, "org_admin").await;
    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "name": "VAST Feed",
            "description": "primary VAST",
            "channel_type": "vast",
            "is_active": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "VAST Feed");
    assert_eq!(body["channel_type"], "vast");
    assert_eq!(body["api_key"].as_str().unwrap().len(), 64);
    app.stop().await;
}

#[tokio::test]
async fn duplicate_channel_name_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice-409").await;
    let org = seed_org(&pool, "acme-409", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;
    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let body = json!({ "name": "feed", "channel_type": "rest" });
    reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url))
        .header("authorization", &token)
        .json(&body)
        .send()
        .await
        .unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url))
        .header("authorization", &token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let rbody: serde_json::Value = resp.json().await.unwrap();
    assert!(rbody["type"].as_str().unwrap().contains("channel_name_taken"));
    app.stop().await;
}

#[tokio::test]
async fn org_isolation_returns_404_when_caller_org_differs() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice-iso").await;
    let org1 = seed_org(&pool, "org-iso-1", "retail").await;
    let org2 = seed_org(&pool, "org-iso-2", "retail").await;
    grant_role(&pool, user, org1, "org_owner").await;
    let cfg = test_config();
    // JWT says org1, but we're querying org2's channels
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations/{org2}/channels", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "feed", "channel_type": "rest" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    app.stop().await;
}

#[tokio::test]
async fn list_get_update_delete_full_lifecycle() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice-lifecycle").await;
    let org = seed_org(&pool, "org-lifecycle", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;
    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;
    let client = reqwest::Client::new();
    let base = format!("{}/api/v1/tenants/organisations/{org}/channels", app.base_url);

    // Create
    let created: serde_json::Value = client
        .post(&base)
        .header("authorization", &token)
        .json(&json!({ "name": "feed", "channel_type": "sensor" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();

    // List
    let list: serde_json::Value = client
        .get(&base)
        .header("authorization", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // Get
    let get_resp = client
        .get(format!("{base}/{id}"))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);

    // Update
    let upd: serde_json::Value = client
        .put(format!("{base}/{id}"))
        .header("authorization", &token)
        .json(&json!({ "name": "renamed", "channel_type": "websocket", "is_active": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(upd["name"], "renamed");
    assert!(!upd["is_active"].as_bool().unwrap());

    // Delete
    let del_resp = client
        .delete(format!("{base}/{id}"))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

    // Get after delete → 404
    let gone = client
        .get(format!("{base}/{id}"))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);

    app.stop().await;
}
```

- [ ] **Step 4: Run HTTP tests**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features tenants_http_channels 2>&1 | tail -30
```

Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tenants/interface.rs tests/tenants_http_channels_test.rs
git commit -m "feat(channels): 5 Axum handlers + HTTP integration tests"
```

---

## Task 14: OpenAPI Registration + Dump

**Files:**
- Modify: `src/openapi.rs`
- Modify: `docs/openapi.json`

- [ ] **Step 1: Register new paths and schemas in `src/openapi.rs`**

In the `paths(...)` list, add:

```rust
crate::tenants::interface::post_create_channel,
crate::tenants::interface::get_list_channels,
crate::tenants::interface::get_channel,
crate::tenants::interface::put_update_channel,
crate::tenants::interface::delete_channel,
```

In the `schemas(...)` list, add:

```rust
crate::tenants::interface::CreateChannelRequest,
crate::tenants::interface::UpdateChannelRequest,
crate::tenants::interface::ChannelBody,
crate::tenants::interface::PagedChannels,
crate::tenants::model::ChannelType,
```

- [ ] **Step 2: Verify compile**

```bash
cargo check
```

- [ ] **Step 3: Regenerate `docs/openapi.json`**

```bash
EGRAS_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras \
  cargo run -- dump-openapi > docs/openapi.json
```

- [ ] **Step 4: Commit**

```bash
git add src/openapi.rs docs/openapi.json
git commit -m "feat(channels): register channel endpoints in OpenAPI spec"
```

---

## Task 15: Final Checks

- [ ] **Step 1: Format check**

```bash
cargo fmt --all -- --check
```

If it fails: `cargo fmt --all` then re-add and amend, or commit a formatting fix.

- [ ] **Step 2: Clippy (warnings are errors)**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Fix any warnings before proceeding.

- [ ] **Step 3: Full test suite**

```bash
TEST_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras_test \
  cargo test --all-features 2>&1 | tail -40
```

Expected: all tests pass, no failures.

- [ ] **Step 4: Final commit (if fmt/clippy produced fixes)**

```bash
git add -p
git commit -m "chore: fmt and clippy fixes for inbound-channels feature"
```
