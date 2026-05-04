---
title: Feature Flags
tags:
  - future-enhancement
  - ops
  - product
---

# Feature Flags

Per-organisation feature flags with overridable defaults. Progressive delivery, A/B testing, and gradual rollouts.

## Schema (Implemented)

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

## First Flag: `auth.api_key_headers`

Seeded in migration 0012:
- **slug:** `auth.api_key_headers`
- **type:** `enum_set`
- **default:** `["x-api-key", "authorization-bearer"]`
- **description:** Which headers carry API keys for this org (subset of supported headers)
- **self_service:** `false` (initially read-only, operator-only override)

Used by the Echo subsystem to determine which HTTP headers are checked for API key authentication.

## Persistence Layer (Implemented)

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

## Future Scope

- **Admin UI** — Read/write UI for org admins to override flags (when `self_service = true`)
- **Client SDKs** — Optional Unleash or OpenFeature client for rule-based evaluation
- **Per-user flags** — Extend to user-level overrides for canary deployments
- **Audit trail** — Record every flag change as an audit event (use [[Audit-System]])
- **Cache + TTL** — In-memory flag cache with periodic refresh to reduce DB load

**Touches:** [[Configuration]], [[Audit-System]], [[Authorization]].
