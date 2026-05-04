---
title: Feature Flags
tags:
  - features
  - multitenancy
  - ops
  - architecture
---

# Feature Flags

Per-organisation feature flags with overridable defaults. Progressive delivery, A/B testing, and gradual rollouts.

## Purpose

Feature flags let operators define a catalog of toggles/values with global defaults, and let individual organisations override those defaults at runtime — without a redeploy. The system is intentionally simple: flags are stored in Postgres, evaluated with an in-memory TTL cache, and exposed over four authenticated REST endpoints.

## Schema

Feature flags are managed via two tables (see [[Data-Model]]):

### `feature_definitions`

Seeded during migrations. Authoritative catalog — app code references slugs as constants. Each flag has:
- **slug** — e.g., `auth.api_key_headers`
- **value_type** — `bool`, `string`, `int`, `enum_set`, or `json`
- **default_value** — JSONB applied globally unless overridden
- **self_service** — whether non-operators can override the flag for their org
- **description** — human-readable

### `organisation_features`

Sparse per-org overrides. If a row exists for `(org_id, slug)`, its `value` is used; otherwise the default applies. Tracks `updated_by` and `updated_at` for audit.

## Permissions

Two permissions control feature flag access (see [[Data-Model]]):
- **`features.read`** — Granted to `org_admin`, `org_owner`. Read flag values for own org. (Not granted to `org_member` — flag values may carry product/strategy hints.)
- **`features.manage`** — Granted to `org_admin`, `org_owner`. Override flags for own org, but only if `self_service = true`; service layer enforces this restriction. Operators (`operator_admin`) bypass `self_service` and can set any flag.

### Self-service semantics

The `self_service` column on `feature_definitions` controls which side of the operator/tenant boundary can write a flag:

| Caller | `self_service = true` | `self_service = false` |
|--------|-----------------------|------------------------|
| Operator (`operator_admin`) | Can read + write | Can read + write |
| Org admin/owner (`features.manage`) | Can read + write | Can read; write → `feature.not_self_service` (403) |
| Org member | No access | No access |

This allows operators to expose tunable knobs to tenants while keeping sensitive infrastructure flags (e.g., `auth.api_key_headers`) operator-only.

## First Flag: `auth.api_key_headers`

Seeded in migration 0012:
- **slug:** `auth.api_key_headers`
- **type:** `enum_set`
- **default:** `["x-api-key", "authorization-bearer"]`
- **description:** Which headers carry API keys for this org (subset of supported headers)
- **self_service:** `false` (operator-only override)

Used by the Echo subsystem to determine which HTTP headers are checked for API key authentication.

## HTTP Surface

All routes require a valid JWT. Cross-tenant access returns 404 (`resource.not_found`) per egras convention. Error responses are RFC 7807 problem documents — see [[Error-Handling]].

| Method | Path | Permission | Description |
|--------|------|------------|-------------|
| `GET` | `/api/v1/features` | `tenants.manage_all` | Operator catalog — list all `FeatureDefinition`s |
| `GET` | `/api/v1/features/orgs/{org_id}` | `features.read` | List effective values for an org (`Vec<EvaluatedFeature>`) |
| `PUT` | `/api/v1/features/orgs/{org_id}/{slug}` | `features.manage` | Set an org override. Body: `{"value": <jsonb>}`. Returns `EvaluatedFeature` with new value + source. |
| `DELETE` | `/api/v1/features/orgs/{org_id}/{slug}` | `features.manage` | Clear org override; effective value reverts to default. |

### Error slugs

| Slug | Status | Trigger |
|------|--------|---------|
| `feature.unknown` | 404 | `slug` not found in `feature_definitions` |
| `feature.not_self_service` | 403 | Non-operator attempts to write a flag with `self_service = false` |
| `feature.invalid_value` | 400 | Supplied `value` does not match the flag's `value_type` |

## Evaluator + Cache Contract

`PgFeatureEvaluator` (in `src/features/service/evaluate.rs`) keeps an in-memory `(org_id, slug) → CachedValue` map with a default TTL of **60 seconds**:

- `evaluate(org_id, slug)` returns the effective value — org override if present, else the global default. Cache is populated on first read; subsequent reads within the TTL skip the database.
- On every write (`set_org_feature` or `clear_org_feature`), the service calls `evaluator.invalidate(org_id, slug)` immediately, so the next `evaluate` call on the same node sees the new value without waiting for TTL expiry.
- `invalidate_all()` flushes the entire cache; used in tests and future admin tooling.

> [!warning] Single-node caveat
> Cache invalidation is **local to the process**. In a multi-node deployment, peer nodes continue serving the stale cached value until their TTL elapses — up to 60 seconds of drift. Cross-node invalidation (event bus / pubsub) is deferred to Future Scope. See [[Configuration]] for the env-var-based config mechanism that will eventually carry bus settings.

## Persistence Layer

`src/features/persistence/` mirrors the standard egras trait/impl split:

- **`feature_repository.rs`** — `FeatureRepository` trait + `FeatureRepoError` (`UnknownSlug`, `Other`).
- **`feature_repository_pg.rs`** — `FeaturePgRepository { pool: PgPool }` with:
  - `list_definitions` / `get_definition` — reads `feature_definitions`, decodes `value_type` via `FeatureValueType::try_from_str`.
  - `list_overrides_for_org` / `get_override` — reads `organisation_features`.
  - `upsert_override` — CTE captures old value before INSERT … ON CONFLICT DO UPDATE; returns `Option<Value>` (None on first insert, Some(old) on update).
  - `delete_override` — DELETE … RETURNING value; zero rows → None.
  - FK violation on slug constraint only (`23503` + `organisation_features_slug_fkey`) → `FeatureRepoError::UnknownSlug`; other FK violations (e.g., bad `organisation_id` or `updated_by`) → `FeatureRepoError::Other`.
  - JSONB columns decoded via `sqlx::types::Json<serde_json::Value>`.

Tests: `tests/it/features_persistence_test.rs` (10 tests, all layers, real Postgres via `TestPool::fresh()`).

## Service Layer

`src/features/service/` implements the business logic for features:

- **`evaluate.rs`** — `FeatureEvaluator` trait + `PgFeatureEvaluator` implementation:
  - In-memory TTL cache (default 60s) reduces DB load
  - `evaluate(org_id, slug)` returns the effective value (override if exists, else default)
  - `invalidate(org_id, slug)` and `invalidate_all()` clear cache entries (called after mutations)

- **`list_definitions.rs`** — `list_definitions(repo)` returns all feature definitions from the catalog

- **`list_org_features.rs`** — `list_org_features(repo, evaluator, org)` returns `Vec<EvaluatedFeature>` showing each flag's effective value and source (default or org-specific override)

- **`set_org_feature.rs`** — `set_org_feature(repo, evaluator, audit, input)`:
  - Validates `value` matches the flag's `value_type`
  - Enforces `self_service` guard: non-operators can only set flags with `self_service = true`
  - Upserts the override in the repository
  - Invalidates the evaluator cache for this org + slug
  - Emits `AuditEvent::feature_set` (if no guard rejection)

- **`clear_org_feature.rs`** — `clear_org_feature(repo, evaluator, audit, input)`:
  - Enforces `self_service` guard (same as set)
  - Deletes the org override (falls back to default)
  - Invalidates the evaluator cache
  - Emits `AuditEvent::feature_cleared` (if no guard rejection)

Tests: `tests/it/features_service_set_test.rs` (7 tests covering happy paths and all rejection scenarios with side-effect assertions).

## Audit Integration

All state-changing operations emit audit events via `AuditRecorder`:

- **`feature.set`** — emitted when `set_org_feature` succeeds. Payload includes:
  - `slug` — the feature slug
  - `old_value` — prior override value or null if new override
  - `new_value` — the new value
  - `self_service` — whether the flag allows org-level self-service override
  - Note: `target_id` is intentionally `None` because features are keyed by slug, not UUID

- **`feature.cleared`** — emitted when `clear_org_feature` succeeds. Payload includes:
  - `slug` — the feature slug
  - `old_value` — the prior override value being removed
  - `self_service` — whether the flag allows org-level self-service override
  - Note: `target_id` is intentionally `None` (same reason as above)

Rejection-path events (e.g., `NotSelfService`, `UnknownSlug`, `InvalidValue`) do not emit audit events.

## Examples

### Consuming `auth.api_key_headers` in Rust

```rust
let feature = state
    .feature_evaluator
    .evaluate(org_id, "auth.api_key_headers")
    .await?;
// feature.value is e.g. ["x-api-key", "authorization-bearer"]
let headers: Vec<String> = serde_json::from_value(feature.value)?;
```

### Operator override via curl (non-self-service flag)

Only an operator JWT can write a flag with `self_service = false`:

```bash
curl -X PUT https://api.example.com/api/v1/features/orgs/{org_id}/auth.api_key_headers \
  -H "Authorization: Bearer $OPERATOR_JWT" \
  -H "Content-Type: application/json" \
  -d '{"value": ["x-api-key"]}'
```

Response: `EvaluatedFeature` with `source: "override"` and the new value.

## Future Scope

- **Cross-node cache invalidation** — Event bus / pubsub to push invalidations to all nodes; see [[Configuration]] for the config mechanism that will carry bus settings
- **Admin UI** — Read/write UI for org admins to override flags (when `self_service = true`)
- **Client SDKs** — Optional Unleash or OpenFeature client for rule-based evaluation
- **Per-user flags** — Extend to user-level overrides for canary deployments
- **Metrics** — Track feature flag usage (which orgs use which flags, frequency of evaluations)
- **Rules engine** — Context-aware evaluation (e.g., flag value depends on user attributes, time of day)

**Touches:** [[Configuration]], [[Audit-System]], [[Authorization]].
