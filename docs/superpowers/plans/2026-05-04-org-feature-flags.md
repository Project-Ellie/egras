# Org-Level Feature Flags — Plan

**Goal:** per-organisation feature flag system with DB-seeded catalog (slug, value_type, default, description, self_service tier), polymorphic JSON values, operator/org-admin write API, audited, in-memory cached evaluator injected via `AppState`. First consumer: `auth.api_key_headers` (controls which header(s) accepted for API-key auth, used by Echo PR next).

**Architecture:**
- New domain `src/features/` (interface/service/model/persistence). Mirrors existing domains.
- Two tables: `feature_definitions` (catalog, seeded by migration; columns immutable from app code except `default_value`/`description`/`self_service`) + `organisation_features` (sparse per-org overrides).
- `FeatureEvaluator` trait on `AppState`, default impl `PgFeatureEvaluator` with in-memory `(org_id, slug) → (value, expires_at)` cache (TTL 60s, invalidated on local write — documented single-node limitation).
- Polymorphic values: catalog declares `value_type ∈ {bool, string, int, enum_set, json}`. PUT validates incoming `value` against `value_type` server-side.
- Two new permissions: `features.read` (org members), `features.manage` (org_admin/org_owner — service layer enforces self_service-only for non-operators; operator bypass via `tenants.manage_all`).
- Audit slugs: `feature.set` (create/update override), `feature.cleared` (DELETE override → revert to default).

**Tech:** Rust, axum, sqlx, serde_json, utoipa, async-trait. No new top-level deps.

---

## File map

**New (Rust):**
- `migrations/0012_features.sql`
- `src/features/mod.rs`
- `src/features/model.rs` — `FeatureValueType`, `FeatureDefinition`, `OrgFeatureOverride`, `EvaluatedFeature`
- `src/features/interface.rs` — handlers + DTOs + router
- `src/features/persistence/mod.rs`
- `src/features/persistence/feature_repository.rs` — trait
- `src/features/persistence/feature_repository_pg.rs`
- `src/features/service/mod.rs`
- `src/features/service/list_definitions.rs` — operator-only catalog
- `src/features/service/list_org_features.rs` — defaults merged with overrides for an org
- `src/features/service/set_org_feature.rs` — write override (validates type + self_service)
- `src/features/service/clear_org_feature.rs` — delete override
- `src/features/service/evaluate.rs` — `FeatureEvaluator` trait + `PgFeatureEvaluator` w/ cache
- `tests/it/features_persistence_test.rs`
- `tests/it/features_service_set_test.rs`
- `tests/it/features_service_list_test.rs`
- `tests/it/features_service_evaluate_test.rs`
- `tests/it/features_http_test.rs`
- `knowledge/wiki/Feature-Flags.md`

**Modify (Rust):**
- `src/lib.rs` — `pub mod features;`
- `src/app_state.rs` — add `feature_evaluator: Arc<dyn FeatureEvaluator>`, add `feature_overrides: Arc<dyn FeatureOverrideRepository>` (or single `features` repo doing both — pick single)
- `src/main.rs` — wire repo + evaluator into `AppState`; mount `features::interface::protected_router()` at the protected mount point
- `src/auth/extractors.rs` — `FeaturesRead`, `FeaturesManage` permission markers
- `src/audit/model.rs` — `feature_set`, `feature_cleared` constructors
- `src/openapi.rs` — register feature DTOs / handlers
- `tests/it/main.rs` — `mod features_persistence_test; mod features_service_*; mod features_http_test;`
- `tests/it/common/` — add helper `seed_feature_definition(...)` if needed for tests
- `docs/openapi.json` — regenerated

**Wiki:**
- `knowledge/wiki/Architecture.md` — add `features/` row mapping to `Feature-Flags.md`
- `knowledge/wiki/future-enhancements/INDEX.md` — strikethrough `Feature-Flags`
- delete: `knowledge/wiki/future-enhancements/Feature-Flags.md` (replaced by promoted note in `wiki/`)

---

## Schema (migrations/0012_features.sql)

```sql
-- Catalog: authoritative list of flags, seeded here. App code references slugs as constants.
CREATE TABLE feature_definitions (
    slug          TEXT PRIMARY KEY,
    value_type    TEXT NOT NULL CHECK (value_type IN ('bool','string','int','enum_set','json')),
    default_value JSONB NOT NULL,
    description   TEXT NOT NULL,
    self_service  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sparse per-org overrides.
CREATE TABLE organisation_features (
    organisation_id UUID NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL REFERENCES feature_definitions(slug) ON DELETE RESTRICT,
    value           JSONB NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID NOT NULL REFERENCES users(id),
    PRIMARY KEY (organisation_id, slug)
);

CREATE INDEX ix_org_features_by_org ON organisation_features (organisation_id);

-- Permissions
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000401', 'features.read',
      'Read feature flag values for own organisation'),
  ('00000000-0000-0000-0000-000000000402', 'features.manage',
      'Set feature flag overrides for own organisation (self_service flags only; operators bypass)')
ON CONFLICT (id) DO NOTHING;

-- Org members (any role) get features.read; org_admin/org_owner get features.manage.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code = 'features.read'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code = 'features.manage'
ON CONFLICT DO NOTHING;

-- Seed the first flag (consumed by Echo PR).
INSERT INTO feature_definitions (slug, value_type, default_value, description, self_service) VALUES
  ('auth.api_key_headers', 'enum_set',
   '["x-api-key","authorization-bearer"]'::jsonb,
   'Which headers carry API keys for this org. Subset of [x-api-key, authorization-bearer].',
   FALSE)
ON CONFLICT (slug) DO NOTHING;
```

---

## Tasks

### Task 1 — Schema + permissions

- [ ] **1.1** Write `migrations/0012_features.sql` per schema above.
- [ ] **1.2** Run `sqlx migrate run` against test DB; sanity-check tables & seeds with `psql`.
- [ ] **1.3** Commit: `feat(features): schema + permissions + seed catalog (auth.api_key_headers)`.

### Task 2 — Model types

Files: `src/features/model.rs`, `src/features/mod.rs`, `src/lib.rs`.

```rust
// src/features/model.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureValueType { Bool, String, Int, EnumSet, Json }

impl FeatureValueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool", Self::String => "string", Self::Int => "int",
            Self::EnumSet => "enum_set", Self::Json => "json",
        }
    }
    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "bool" => Self::Bool, "string" => Self::String, "int" => Self::Int,
            "enum_set" => Self::EnumSet, "json" => Self::Json, _ => return None,
        })
    }
    /// Returns Err with reason if `v` does not match this declared type.
    pub fn validate(&self, v: &Value) -> Result<(), &'static str> {
        match (self, v) {
            (Self::Bool, Value::Bool(_))                            => Ok(()),
            (Self::String, Value::String(_))                        => Ok(()),
            (Self::Int, Value::Number(n)) if n.is_i64()             => Ok(()),
            (Self::EnumSet, Value::Array(arr))
                if arr.iter().all(|x| x.is_string())                => Ok(()),
            (Self::Json, _)                                         => Ok(()),
            (Self::Bool, _)     => Err("expected boolean"),
            (Self::String, _)   => Err("expected string"),
            (Self::Int, _)      => Err("expected integer"),
            (Self::EnumSet, _)  => Err("expected array of strings"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDefinition {
    pub slug: String,
    pub value_type: FeatureValueType,
    pub default_value: Value,
    pub description: String,
    pub self_service: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgFeatureOverride {
    pub organisation_id: Uuid,
    pub slug: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Uuid,
}

/// Effective value for an (org, slug) pair, with provenance for UI/audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatedFeature {
    pub slug: String,
    pub value: Value,
    pub source: FeatureSource, // Default | Override
    pub value_type: FeatureValueType,
    pub self_service: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSource { Default, Override }
```

- [ ] **2.1** Write `src/features/model.rs` (above).
- [ ] **2.2** Write `src/features/mod.rs`:
  ```rust
  pub mod model;
  pub mod interface;
  pub mod persistence;
  pub mod service;
  ```
- [ ] **2.3** Add `pub mod features;` to `src/lib.rs`.
- [ ] **2.4** `cargo build` — green.
- [ ] **2.5** Commit: `feat(features): model types — FeatureDefinition, OrgFeatureOverride, EvaluatedFeature`.

### Task 3 — Repository (trait + Postgres + tests, TDD)

Files: `src/features/persistence/{mod,feature_repository,feature_repository_pg}.rs`, `tests/it/features_persistence_test.rs`.

Trait surface:

```rust
// src/features/persistence/feature_repository.rs
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::features::model::{FeatureDefinition, OrgFeatureOverride};

#[derive(Debug, thiserror::Error)]
pub enum FeatureRepoError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[async_trait]
pub trait FeatureRepository: Send + Sync + 'static {
    async fn list_definitions(&self) -> Result<Vec<FeatureDefinition>, FeatureRepoError>;
    async fn get_definition(&self, slug: &str) -> Result<Option<FeatureDefinition>, FeatureRepoError>;
    async fn list_overrides_for_org(&self, org: Uuid) -> Result<Vec<OrgFeatureOverride>, FeatureRepoError>;
    async fn get_override(&self, org: Uuid, slug: &str) -> Result<Option<OrgFeatureOverride>, FeatureRepoError>;
    /// Upserts. Returns previous value (if any) for audit.
    async fn upsert_override(
        &self, org: Uuid, slug: &str, value: Value, updated_by: Uuid,
    ) -> Result<Option<Value>, FeatureRepoError>;
    /// Deletes if present. Returns previous value (if any) for audit.
    async fn delete_override(
        &self, org: Uuid, slug: &str,
    ) -> Result<Option<Value>, FeatureRepoError>;
}
```

- [ ] **3.1** Write `feature_repository.rs` (above) and `persistence/mod.rs` re-exporting trait + pg impl.
- [ ] **3.2** Write `tests/it/features_persistence_test.rs` covering: insert override → read back; upsert returns old value; delete returns old value; list-overrides; unknown-slug rejection (FK violation surfaces as error). Use existing `TestPool::fresh()` pattern.
- [ ] **3.3** Run; expect FAIL.
- [ ] **3.4** Implement `feature_repository_pg.rs` (`FeaturePgRepository { pool: PgPool }`, raw `sqlx::query_as` / `query_scalar`, JSON columns via `Json<Value>`). Translate FK violations on `slug` → `UnknownSlug`.
- [ ] **3.5** Run; expect PASS.
- [ ] **3.6** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] **3.7** Commit: `feat(features): persistence — repository trait + Postgres impl`.

### Task 4 — Evaluator (cache, TDD)

Files: `src/features/service/evaluate.rs`, `src/features/service/mod.rs`, `tests/it/features_service_evaluate_test.rs`.

```rust
// Excerpt — full file in implementation.
#[async_trait]
pub trait FeatureEvaluator: Send + Sync + 'static {
    async fn evaluate(&self, org: Uuid, slug: &str) -> Result<Value, EvaluateError>;
    async fn invalidate(&self, org: Uuid, slug: &str);
    async fn invalidate_all(&self);
}

pub struct PgFeatureEvaluator {
    repo: Arc<dyn FeatureRepository>,
    cache: Arc<RwLock<HashMap<(Uuid, String), CachedValue>>>,
    ttl: Duration, // default 60s; override via FeatureEvaluatorConfig
}
struct CachedValue { value: Value, expires_at: Instant }
```

Evaluation order: cache hit → return; miss → repo `get_override(org, slug)` → if Some, use override value; else `get_definition(slug)` → use `default_value`; cache with TTL; return. Unknown slug → `EvaluateError::UnknownSlug` (do NOT cache misses).

- [ ] **4.1** Write `tests/it/features_service_evaluate_test.rs` — known slug w/o override returns default; with override returns overridden; invalidate clears cache (test by changing repo state behind evaluator and confirming staleness, then invalidate, then fresh).
- [ ] **4.2** Run; FAIL.
- [ ] **4.3** Implement `evaluate.rs` — `tokio::sync::RwLock`, `std::time::Instant`. Default TTL 60s. Cache stored by `(Uuid, String)`.
- [ ] **4.4** Wire `pub mod evaluate;` in `service/mod.rs`. Re-export `FeatureEvaluator` from `src/features/mod.rs`.
- [ ] **4.5** PASS, fmt, clippy.
- [ ] **4.6** Commit: `feat(features): evaluator with TTL cache and invalidation`.

### Task 5 — Service: list + set + clear (TDD)

Files: `src/features/service/{list_definitions,list_org_features,set_org_feature,clear_org_feature}.rs`, `tests/it/features_service_set_test.rs`, `tests/it/features_service_list_test.rs`.

Inputs (sketch):

```rust
pub struct SetOrgFeatureInput {
    pub organisation_id: Uuid,
    pub slug: String,
    pub value: Value,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
    pub actor_is_operator: bool, // service layer is permission-blind otherwise
}

#[derive(Debug, thiserror::Error)]
pub enum SetOrgFeatureError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error("flag is not self_service; operator privileges required")]
    NotSelfService,
    #[error("value does not match declared type: {0}")]
    InvalidValue(&'static str),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

Behaviour:
- `list_definitions(state)` → `Vec<FeatureDefinition>` from repo. Used by operators only; HTTP enforces.
- `list_org_features(state, org_id)` → `Vec<EvaluatedFeature>` — for each definition, check override; emit `EvaluatedFeature` with source.
- `set_org_feature(state, input)` —
  1. Load definition; `UnknownSlug` if missing.
  2. Validate `value` against `value_type`.
  3. If `!definition.self_service && !input.actor_is_operator` → `NotSelfService`.
  4. `upsert_override` (returns old value).
  5. `evaluator.invalidate(org, slug)`.
  6. Audit `feature_set` with `{slug, old_value, new_value, self_service}`.
- `clear_org_feature(state, input)` — `delete_override`, invalidate, audit `feature_cleared` with `{slug, old_value}`.

- [ ] **5.1** Write `tests/it/features_service_set_test.rs` — happy path (operator sets non-self-service); reject non-self-service for non-operator (`NotSelfService`); reject type mismatch (`InvalidValue`); reject unknown slug. Use `BlockingAuditRecorder` to assert audit emitted.
- [ ] **5.2** Write `tests/it/features_service_list_test.rs` — definitions; org features (mix of overridden + default sources).
- [ ] **5.3** Run; FAIL.
- [ ] **5.4** Implement the four service files. Each follows the `create_service_account` shape (state ref, input struct, typed error, returns domain type).
- [ ] **5.5** Add `feature_set` and `feature_cleared` constructors to `src/audit/model.rs`. Use `AuditCategory::TenantsStateChange` (closest fit; flags are tenant configuration). Event types: `feature.set`, `feature.cleared`. `target_type = "feature"`. Payload: `{slug, old_value: <opt>, new_value: <opt>, self_service: bool}`.
- [ ] **5.6** PASS, fmt, clippy.
- [ ] **5.7** Commit: `feat(features): service layer — list, set, clear with audit and cache invalidation`.

### Task 6 — AppState wiring + permissions extractors

Files: `src/app_state.rs`, `src/auth/extractors.rs`, `src/main.rs`.

- [ ] **6.1** Add to `AppState`:
  ```rust
  pub features: Arc<dyn FeatureRepository>,
  pub feature_evaluator: Arc<dyn FeatureEvaluator>,
  ```
  Update all `AppState` construction sites (main.rs, testing helpers).
- [ ] **6.2** Add to `src/auth/extractors.rs`:
  ```rust
  pub struct FeaturesRead;
  impl Permission for FeaturesRead {
      const CODE: &'static str = "features.read";
      fn accepts(set: &PermissionSet) -> bool {
          set.has(Self::CODE) || set.is_operator_over_tenants()
      }
  }
  pub struct FeaturesManage;
  impl Permission for FeaturesManage {
      const CODE: &'static str = "features.manage";
      fn accepts(set: &PermissionSet) -> bool {
          set.has(Self::CODE) || set.is_operator_over_tenants()
      }
  }
  ```
- [ ] **6.3** Update `src/main.rs` to construct `Arc<PgFeatureEvaluator>` from `Arc<FeaturePgRepository>`, inject both.
- [ ] **6.4** Update `src/testing.rs` `AppState` builder to inject test impls (real `PgFeatureEvaluator` over `TestPool`).
- [ ] **6.5** `cargo build && cargo clippy --all-targets --all-features -- -D warnings` — green.
- [ ] **6.6** Commit: `feat(features): wire repository + evaluator into AppState; add permission extractors`.

### Task 7 — HTTP interface (TDD)

Files: `src/features/interface.rs`, `tests/it/features_http_test.rs`.

Routes (mounted under the protected router at `/v1`):
- `GET    /orgs/{org_id}/features` — `FeaturesRead`. Returns `Vec<EvaluatedFeature>`.
- `PUT    /orgs/{org_id}/features/{slug}` — `FeaturesManage`. Body `{"value": <jsonb>}`. Returns 200 with the new `EvaluatedFeature`.
- `DELETE /orgs/{org_id}/features/{slug}` — `FeaturesManage`. Returns 204.
- `GET    /features` — `tenants.manage_all` only (operator catalog view). Returns `Vec<FeatureDefinition>`.

Cross-tenant protection: caller's `actor_org_id` must equal `org_id`, OR caller is operator (`is_operator_over_tenants()`). Mismatch → 404 (hide existence), per egras convention.

Service-layer `actor_is_operator` is set from the extractor's `PermissionSet::is_operator_over_tenants()`.

- [ ] **7.1** Write `tests/it/features_http_test.rs`:
  - GET own-org features as member → 200 with full list (defaults).
  - GET other-org features as non-operator → 404.
  - PUT own-org self_service flag as org_admin → 200, value reflected in subsequent GET.
  - PUT own-org non-self-service flag as org_admin → 403 (RFC 7807, slug `feature.not_self_service` — define on `AppError`).
  - PUT any flag as operator → 200.
  - PUT with type-mismatched value → 400 (slug `feature.invalid_value`).
  - PUT unknown slug → 404 (slug `feature.unknown`).
  - DELETE own-org override → 204; subsequent GET shows source=default.
- [ ] **7.2** Add error variants + slugs to `src/errors.rs`:
  ```rust
  AppError::FeatureUnknown,            // slug "feature.unknown"
  AppError::FeatureNotSelfService,     // slug "feature.not_self_service"
  AppError::FeatureInvalidValue(String), // slug "feature.invalid_value"
  ```
  Map → 404 / 403 / 400 respectively in `IntoResponse` impl.
- [ ] **7.3** Run tests; FAIL.
- [ ] **7.4** Implement `interface.rs`:
  - Handlers `get_org_features`, `put_org_feature`, `delete_org_feature`, `get_definitions`.
  - DTOs with `ToSchema` for utoipa.
  - `protected_router() -> Router<AppState>` returning the four routes.
- [ ] **7.5** Mount the router at the protected mount site in `main.rs`.
- [ ] **7.6** Register utoipa paths in `src/openapi.rs`.
- [ ] **7.7** Run tests; PASS.
- [ ] **7.8** `cargo run -- dump-openapi > docs/openapi.json`.
- [ ] **7.9** fmt, clippy, full nextest. Commit: `feat(features): HTTP API — list, set, clear, catalog`.

### Task 8 — Wiki

- [ ] **8.1** Create `knowledge/wiki/Feature-Flags.md`. Cover: purpose, schema, permissions model, self_service tier semantics, evaluator + cache + invalidation contract (single-node TTL caveat, link to `Configuration` for future cross-node bus), HTTP surface, audit slugs, examples (`auth.api_key_headers`).
- [ ] **8.2** Edit `knowledge/wiki/Architecture.md` — add `features/ → Feature-Flags.md`.
- [ ] **8.3** Edit `knowledge/wiki/Configuration.md` — add a "See also" pointer to `Feature-Flags.md` (per-org runtime overrides complement env-var configuration).
- [ ] **8.4** Edit `knowledge/wiki/future-enhancements/INDEX.md` — strikethrough `Feature-Flags`.
- [ ] **8.5** `git rm knowledge/wiki/future-enhancements/Feature-Flags.md`.
- [ ] **8.6** Commit: `docs(wiki): promote Feature-Flags to shipped`.

### Task 9 — Update Echo plan to consume the flag

- [ ] **9.1** Edit `docs/superpowers/plans/2026-05-03-echo-and-notebook-harness.md`:
  - Replace Q1 in "Unresolved questions" with: *"Resolved: API-key header is governed by per-org flag `auth.api_key_headers` (enum_set). Echo PR depends on Feature-Flags PR (2026-05-04). Default `["x-api-key","authorization-bearer"]` allows both."*
  - Add a new task between Task 1 and Task 2: *"Task 1.5 — API-key middleware reads `auth.api_key_headers` flag for the org of the resolved key; reject (401) if the header used to present the key is not in the allowlist."*
  - Add a notebook step (in Task 4.1) demonstrating: PUT `/v1/orgs/{id}/features/auth.api_key_headers` to `["x-api-key"]`, then assert that an `Authorization: Bearer <key>` request gets 401, while `X-API-Key: <key>` still works.
- [ ] **9.2** Commit: `docs(plans): make echo plan depend on org feature flags`.

### Task 10 — Pre-push gate + push + PR

- [ ] **10.1** `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run --all-features` — green.
- [ ] **10.2** Branch `feat/org-feature-flags`, push, open PR titled `feat: org-level feature flags`.
- [ ] **10.3** Poll CI; iterate.

---

## Out of scope (deferred)
- Cross-node cache invalidation (event bus / pubsub). Single-node TTL only; document in wiki.
- Org-admin UI surface (front-end). API only.
- Bulk PUT, scheduled rollouts, percentage-based rollouts (Unleash/OpenFeature territory). Per the wiki draft these belong to a later phase.
- Audit log of catalog changes (default_value/description tweaks via migration). Migrations are reviewed via PR; no in-app audit needed in v1.
- Caller-side ergonomic helper macro (`feature!("slug")`). Add when there are 3+ consumers.

---

## Decisions (resolved 2026-05-04)
1. **`int` value type:** included. Cheap insurance.
2. **`features.read`:** `org_admin` + `org_owner` only. Members do not see flag values; reduces info-leak surface.
3. **Evaluator TTL:** 60s. Will tune with telemetry once load is observable.
4. **Catalog mutation:** migration-only. Slug stability is a hard contract; code references slugs as constants. Runtime PUT to add definitions is rejected.
5. **Wiki placement:** flat at `knowledge/wiki/Feature-Flags.md`, with a link added from the existing `knowledge/wiki/Configuration.md`. Matches established flat convention; no new sub-directory created for a single note.
