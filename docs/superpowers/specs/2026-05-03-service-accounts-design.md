---
title: Service Accounts & API Keys — Design
date: 2026-05-03
status: draft
tags:
  - spec
  - service-accounts
  - api-keys
  - auth
---

# Service Accounts & API Keys — Design Spec

## Why

B2B integrators need non-human principals: long-lived credentials, scoped permissions, no email/password lifecycle, no JWT-from-login. Today every authenticated request must come from a logged-in `User`. This spec adds **service accounts** as a kind of principal and **API keys** as their credential, integrating with the existing RBAC and `AuthLayer` rather than forking them.

Distinct from the [`InboundChannel.api_key`](../../../src/tenants/persistence/channel_repository_pg.rs) — those keys identify integration *endpoints* for incoming traffic; service-account keys identify *callers* in our auth model.

## Non-goals (v1)

- No per-key TTL / expiry. Rotation = create new + revoke old.
- No per-key IP allowlist or rate limit (covered by [[future-enhancements/Rate-Limiting-and-Quotas]]).
- No JWT-exchange flow (`POST /token` with key). Keys are used directly.
- No SA-managing-SA. Humans only create / rotate / revoke SAs and keys.
- No cross-org SAs. An SA lives in one org for life.
- No org-wide "list all SA keys" endpoint (per-SA list is enough).
- No `via_api_key_id` audit metadata (subject_id = SA user_id is sufficient for v1).

## Principal model — hybrid

Three confirmed design axes:

1. **Hybrid principal**: SAs share `users.id` (so RBAC/audit reuse for free) + sidecar `service_accounts` table for SA-only metadata.
2. **API key as Bearer credential**: prefix-discriminated, `AuthLayer` dispatches on `egras_*` prefix; both paths produce uniform `Claims` + `PermissionSet` extensions.
3. **Optional `scopes` per key, intersected with SA perms** (`NULL = inherit`; empty array rejected at service layer).

## Data model — migration `0011_service_accounts.sql`

```sql
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
    prefix                   TEXT NOT NULL UNIQUE,           -- 8 hex chars
    secret_hash              TEXT NOT NULL,                  -- argon2 of secret
    name                     TEXT NOT NULL,
    scopes                   TEXT[],                         -- NULL => inherit
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by               UUID NOT NULL REFERENCES users(id),
    last_used_at             TIMESTAMPTZ,
    revoked_at               TIMESTAMPTZ,
    CHECK (scopes IS NULL OR cardinality(scopes) > 0)
);

CREATE INDEX ix_api_keys_active_by_sa
    ON api_keys (service_account_user_id) WHERE revoked_at IS NULL;
```

Existing `UserRepository::create` continues writing `kind = 'human'` (default). The SA service writes `kind = 'service_account'` in the same tx as the `service_accounts` insert.

`ON DELETE CASCADE` on both FKs means deleting the user row collapses everything cleanly; `user_organisation_roles` already cascades on user delete in the existing schema.

## Permission codes

Two new codes, seeded in `0011_service_accounts.sql` alongside the schema (one migration, one PR — historical migrations stay frozen):

| Code | Granted to | Allows |
|---|---|---|
| `service_accounts.read` | org_admin, org_owner | List + read SAs and key metadata in own org |
| `service_accounts.manage` | org_admin, org_owner | Create / delete SAs; create / rotate / revoke keys |

Single `manage` for v1 — split later if asked.

## Module layout

Lives entirely under `security/` (SAs are principals → security domain).

```
src/security/
├── model.rs                       (+ UserKind, ServiceAccount, ApiKey, ApiKeyMaterial, NewApiKey;
│                                    User struct gains `kind: UserKind`)
├── persistence/
│   ├── service_account_repository{,_pg}.rs    (NEW)
│   ├── api_key_repository{,_pg}.rs            (NEW)
│   └── ...                                    (existing user/token unchanged)
├── service/
│   ├── create_service_account.rs              (NEW)
│   ├── list_service_accounts.rs               (NEW — paginated, org-scoped)
│   ├── delete_service_account.rs              (NEW)
│   ├── create_api_key.rs                      (NEW — returns plaintext ONCE)
│   ├── list_api_keys.rs                       (NEW — metadata only)
│   ├── revoke_api_key.rs                      (NEW)
│   └── rotate_api_key.rs                      (NEW — atomic create+revoke in 1 tx)
└── interface.rs                   (handlers + DTOs appended)
```

## Auth path

### Key format

```
egras_<env>_<prefix8>_<secret_b64>
       │      │         │
       │      │         └─ 32 random bytes, base64url-no-pad (43 chars)
       │      └─ first 8 hex chars from random 4 bytes (= api_keys.prefix, UNIQUE)
       └─ "live" (hard-coded for v1; reserved for future test mode)
```

Total length ≈ 65 chars. `prefix8` is the lookup token; `secret_b64` is verified with Argon2 against `secret_hash`.

Generation: 4 random bytes → hex → `prefix`; 32 random bytes → b64url → `secret`. The `id` is independent (UUID v7). On `INSERT` the `prefix` UNIQUE constraint catches the (~1 in 4 B per insert) collision — service retries once with a fresh prefix on `unique_violation`.

### `AuthLayer` extension

A new strategy alongside the existing `PermissionLoader` and `RevocationChecker`:

```rust
#[async_trait]
pub trait ApiKeyVerifierStrategy: Send + Sync + 'static {
    async fn verify(&self, prefix: &str, secret: &str)
        -> anyhow::Result<Option<VerifiedKey>>;
}
pub struct VerifiedKey {
    pub key_id: Uuid,
    pub sa_user_id: Uuid,
    pub organisation_id: Uuid,
    pub scopes: Option<Vec<String>>,
}
```

`PgApiKeyVerifier` implementation lives in `src/security/persistence/` (the verifier *uses* domain knowledge; the `auth/` module stays domain-agnostic). Wired in `build_app` exactly like `PgPermissionLoader`.

`AuthService::call` body:

```text
extract Authorization: Bearer
match token.starts_with("egras_") {
    true:
        parse env|prefix8|secret  →  ApiKeyVerifier.verify(prefix8, secret)
        on hit:
            sa_perms        = PermissionLoader.load(sa_user_id, org_id)
            effective_perms = scopes.map(|s| sa_perms.intersect(&s)).unwrap_or(sa_perms)
            schedule throttled UPDATE api_keys.last_used_at (best-effort, non-blocking)
            insert Claims { sub: sa_user_id, org: org_id, jti: deterministic-from-key_id }
            insert PermissionSet(effective_perms)
            insert Caller::ApiKey { key_id, sa_user_id, org_id }
            // skip RevocationChecker — revocation is api_keys.revoked_at, already filtered
        on miss: 401 reason=invalid_api_key
    false:
        existing JWT path
        insert Claims, PermissionSet, Caller::User { user_id, org_id, jti }
}
```

The synthesized `Claims` keep every existing handler working without migration. The `Caller` enum is new (in `src/auth/extractors.rs`) and is what new handlers extract when they need to differentiate.

`Claims.exp` for the synthesized variant: set to request_time + 1y (cosmetic; never re-validated post-middleware).

### Last-used update — throttled

After successful auth:

```sql
UPDATE api_keys
   SET last_used_at = NOW()
 WHERE id = $1
   AND (last_used_at IS NULL OR last_used_at < NOW() - INTERVAL '60 seconds');
UPDATE service_accounts
   SET last_used_at = NOW()
 WHERE user_id = $2
   AND (last_used_at IS NULL OR last_used_at < NOW() - INTERVAL '60 seconds');
```

≤ 1 write per minute per key/SA regardless of QPS. Failure logs and continues — best-effort, never fails the request. **Known limit:** under sustained high QPS on a single key, all callers still take the row lock briefly. If observed, migrate to an mpsc-channel + worker (audit-pipeline pattern). Out of scope for v1.

## Caller-type gating

`Caller` enum surfaced in extensions. Five categories of existing endpoints reject `Caller::ApiKey` with **403 + slug `requires_user_credentials`**:

| Category | Endpoints |
|---|---|
| Identity lifecycle | `POST /security/logout`, `POST /security/change-password`, `POST /security/password-reset/request`, `POST /security/password-reset/confirm` |
| Org switching | `POST /security/switch-org` |
| **SA / key management** | every endpoint listed below in the v1 surface |

The last category prevents key-pivot escalation: a stolen key cannot mint more keys.

Implementation: a `RequireHumanCaller` extractor (sibling to `Perm<P>`) added in `src/auth/extractors.rs`. Each handler that needs the gate adds the extractor — no middleware, no implicit behavior.

## Cross-org SA role assignments — forbidden

Service layer of `assign_role`:

```rust
if target_user.kind == UserKind::ServiceAccount
    && target.organisation_id != service_account.organisation_id {
    return Err(AssignRoleError::ServiceAccountCrossOrgForbidden);
}
```

Returned as **400 + slug `service_account_cross_org_forbidden`**. SAs are bound to their home org; preserves the "one SA = one customer" mental model.

## v1 endpoint surface

All require `Caller::User`. CRUD targets are 404 (not 403) for cross-org access — same convention as tenants.

```
POST   /api/v1/security/service-accounts                              [perm: service_accounts.manage]
GET    /api/v1/security/service-accounts                              [perm: service_accounts.read]
GET    /api/v1/security/service-accounts/{sa_id}                      [perm: service_accounts.read]
DELETE /api/v1/security/service-accounts/{sa_id}                      [perm: service_accounts.manage]

POST   /api/v1/security/service-accounts/{sa_id}/api-keys             [perm: service_accounts.manage]
GET    /api/v1/security/service-accounts/{sa_id}/api-keys             [perm: service_accounts.read]
DELETE /api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}    [perm: service_accounts.manage]
POST   /api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}/rotate  [perm: service_accounts.manage]
```

`POST .../api-keys` returns the **plaintext key once** in the response body under `key`; subsequent `GET` returns metadata only (`prefix`, `name`, `scopes`, `created_at`, `last_used_at`, `revoked_at`).

Role assignment to SAs reuses the existing `POST /tenants/{org_id}/users/{user_id}/roles` (no new endpoint).

## Audit events

Emitted by the new handlers — same pattern as existing security events:

| Event type | When | Subject |
|---|---|---|
| `service_account.created` | SA created | `subject_id = sa_user_id`, `actor_id = caller.user_id` |
| `service_account.deleted` | SA deleted (cascades keys) | same |
| `api_key.created` | New key minted | same; metadata `key_id`, `prefix` |
| `api_key.revoked` | Key revoked (explicit or via rotate) | same |
| `api_key.rotated` | Key rotated (atomic create+revoke) | same; metadata `old_key_id`, `new_key_id` |

## Tests — vertical slices

### Persistence (`tests/it/security_persistence_service_accounts_test.rs`)

1. Create SA + read back: `users.kind = 'service_account'`, sidecar row visible.
2. Create SA twice with same name in same org → unique violation; same name in *different* org → ok.
3. Create api_key; lookup-by-prefix returns it; revoked key is not returned.
4. `last_used` UPDATE only fires when 60 s have elapsed (immediate second write is a no-op).

### Service (`tests/it/security_service_service_accounts_test.rs`)

5. Create SA happy path emits `service_account.created` audit event with `actor_id = creator`.
6. Create api_key returns plaintext `key` once; storage holds only `secret_hash`.
7. Rotate key creates new active key + revokes old in one tx; emits `api_key.rotated` audit event.
8. Cross-org caller (operator-org bypass excluded) gets 404 on SA endpoints.
9. `assign_role` to SA in foreign org returns `ServiceAccountCrossOrgForbidden`.

### HTTP (`tests/it/security_http_service_accounts_test.rs`)

10. End-to-end: human creates SA → creates api_key → uses key as `Bearer egras_live_...` → request authenticates with PermissionSet matching SA's perms.
11. Restricted-scope key (`scopes = ["audit.read"]`) cannot reach a perm absent from its scope set, even if the SA has it. Returns 403.
12. Revoked key returns 401 with `reason: invalid_api_key`.
13. API-key caller hits `POST /security/logout` → 403 `requires_user_credentials`.
14. API-key caller hits SA-management endpoint → 403 `requires_user_credentials`.

## Wiring (`build_app`)

```rust
let api_keys = Arc::new(ApiKeyRepositoryPg::new(pool.clone()));
let service_accounts = Arc::new(ServiceAccountRepositoryPg::new(pool.clone()));

let api_key_verifier = ApiKeyVerifier::pg(api_keys.clone());

// AuthLayer constructor gains a fourth arg
let auth_layer = AuthLayer::new(secret, issuer, perm_loader, revocation, api_key_verifier);

// AppState gains the two repos so handlers can use them
state.api_keys = api_keys;
state.service_accounts = service_accounts;
```

`MockAppStateBuilder` gains `with_pg_service_account_repos()` (pattern matches existing `with_pg_security_repos()`).

## Wiki updates (same PR)

- New: `knowledge/wiki/Service-Accounts.md` — feature overview, lifecycle, key-format docs, integration guide.
- Update: `knowledge/wiki/Architecture.md` — add `service_account_repository.rs`, `api_key_repository.rs`, the new service files; mention `Caller` enum.
- Update: `knowledge/wiki/Authentication.md` — document the prefix-dispatch path in `AuthLayer`, the `ApiKeyVerifier` strategy, `Caller` enum.
- Update: `knowledge/wiki/Authorization.md` — document scope-intersection semantics; `RequireHumanCaller` extractor; `service_accounts.{read,manage}` permission codes.
- Update: `knowledge/wiki/Security-Domain.md` — service-account use cases.
- Update: `knowledge/wiki/Data-Model.md` — `users.kind`, `service_accounts`, `api_keys`.
- Update: `knowledge/wiki/future-enhancements/INDEX.md` — strike through `Service-Accounts-and-API-Keys`.
- Delete: `knowledge/wiki/future-enhancements/Service-Accounts-and-API-Keys.md`.

## Open questions

(All resolved during brainstorm — recorded for posterity.)

- *Principal model?* → Hybrid: `users` row + sidecar `service_accounts`.
- *Credential transport?* → Bearer with prefix dispatch in `AuthLayer`.
- *Permission semantics?* → Optional `scopes` per key, intersected with SA's perms (`NULL = inherit`, empty array rejected).
- *`env` in prefix?* → Keep, hard-coded `live` for v1.
- *Audit on revoke?* → Yes; `api_key.revoked` and `api_key.rotated` events.
- *Permission code split?* → Single `service_accounts.manage` for v1.
- *Org-wide key listing?* → Out of v1.
- *Human-only operations?* → Logout, change-password, password-reset, switch-org, all SA/key management. Reject API-key with 403 `requires_user_credentials`.
- *Cross-org SA role assignment?* → Forbidden. 400 `service_account_cross_org_forbidden`.
