---
title: Service Accounts & API Keys
tags:
  - service-accounts
  - api-keys
  - auth
  - architecture
---

# Service Accounts & API Keys

Non-human principals for B2B integrations. Lives at `src/security/` (SAs are a kind of principal — security domain) and integrates with the existing RBAC + `AuthLayer` rather than forking either.

## Why

Long-lived server-to-server credentials need: scoped permissions, no email/password lifecycle, no JWT-from-login, audit and revocation as first-class operations. Today every authenticated request must come from a logged-in `User`; this feature adds a parallel principal type with API-key Bearer auth.

Distinct from [`InboundChannel.api_key`](../../src/tenants/persistence/channel_repository_pg.rs) — channel keys identify *integration endpoints* receiving traffic; service-account keys identify *callers* in our auth model.

## Hybrid principal model

```
users (id, kind = 'human' | 'service_account', ...)
  ▲
  │ ON DELETE CASCADE
  │
service_accounts (user_id PK FK -> users, organisation_id, name, ...)
  ▲
  │ ON DELETE CASCADE
  │
api_keys (id, service_account_user_id FK, prefix, secret_hash, scopes, ...)
```

- Service accounts share `users.id` so existing role assignments (`user_organisation_roles`) and audit subject_ids work for free.
- A sidecar `service_accounts` row holds SA-only metadata (creator, name, last-used) without polluting `users` with NULL columns.
- API keys are children of an SA. Many keys per SA; one secret hash per key.

## Module layout

| File | Role |
|------|------|
| [`src/security/model.rs`](../../src/security/model.rs) | `UserKind`, `ServiceAccount`, `ApiKey`, `ApiKeyMaterial`, `NewApiKey` |
| [`src/security/persistence/service_account_repository.rs`](../../src/security/persistence/service_account_repository.rs) | `ServiceAccountRepository` trait |
| [`src/security/persistence/service_account_repository_pg.rs`](../../src/security/persistence/service_account_repository_pg.rs) | Postgres impl |
| [`src/security/persistence/api_key_repository.rs`](../../src/security/persistence/api_key_repository.rs) | `ApiKeyRepository` trait |
| [`src/security/persistence/api_key_repository_pg.rs`](../../src/security/persistence/api_key_repository_pg.rs) | Postgres impl + `PgApiKeyVerifier` |
| [`src/security/service/api_key_secret.rs`](../../src/security/service/api_key_secret.rs) | Wire format generation / parse / hash / verify |
| [`src/security/service/{create,list,delete}_service_account.rs`](../../src/security/service/) | SA use cases |
| [`src/security/service/{create,list,revoke,rotate}_api_key.rs`](../../src/security/service/) | API key use cases |
| [`migrations/0011_service_accounts.sql`](../../migrations/0011_service_accounts.sql) | Schema + permission seeds |

## API key wire format

```
egras_<env>_<prefix8>_<secret_b64>
       │      │         │
       │      │         └─ 32 random bytes, base64url-no-pad (43 chars)
       │      └─ first 8 hex chars from random 4 bytes = api_keys.prefix UNIQUE
       └─ "live" (hard-coded for v1; reserved for future test mode)
```

≈65 chars total. The `prefix8` is the lookup token; the secret is verified with Argon2 against `secret_hash`. `prefix` is UNIQUE so the AuthLayer can find the row in O(log N) without scanning.

## Auth path

The `AuthLayer` sniffs the `Authorization: Bearer ...` header. Tokens starting with `egras_` go to the API-key path; everything else goes to JWT. Both produce the same `Claims` + `PermissionSet` extensions, so existing handlers + `Perm<P>` extractors work unchanged.

```
                Bearer xxx
                    │
         starts_with("egras_")?
            │              │
           yes             no
            ▼              ▼
   ApiKeyVerifier      decode JWT
       .verify          .is_revoked
            │              │
            ▼              ▼
     load PermissionSet ──┴──▶ (sa_perms ∩ scopes if api-key)
            │
            ▼
   insert Claims, PermissionSet, Caller into request extensions
```

For API-key auth, `Claims` is **synthesised**: `sub = sa_user_id`, `org = sa.organisation_id`, `jti = deterministic-from-key.id`, `exp = now + 1y`. Revocation lives at `api_keys.revoked_at` (already filtered in `verify`); the JWT `revoked_tokens` table is not consulted for API-key requests.

## Permission scoping per key

Each API key has an optional `scopes: TEXT[]`:

- `NULL` → inherit all of the SA's permissions.
- `["service_accounts.read", "audit.read_own_org"]` → restrict.
- `[]` → rejected at the service layer (use `NULL` to inherit).

The middleware computes `effective = sa_perms ∩ scopes` (via `PermissionSet::intersect`). This means **stripping a role from the SA also strips it from every key** — keys cannot exceed their parent.

## `Caller` enum + `RequireHumanCaller`

`AuthLayer` inserts `Caller::User { user_id, org_id, jti }` or `Caller::ApiKey { key_id, sa_user_id, org_id }` into request extensions. Handlers that need to differentiate extract `Caller`. Handlers that must reject API-key callers use `RequireHumanCaller` — a zero-sized extractor that returns 403 `auth.requires_user_credentials` for API-key calls.

Endpoints gated to humans only (full list):

| Reason | Endpoints |
|---|---|
| Identity lifecycle | `POST /security/register`, `/logout`, `/change-password`, `/switch-org` |
| Pivot-escalation guard | every `/security/service-accounts/*` endpoint (a stolen key cannot mint more keys) |

## Cross-org SA constraint

SAs are bound to their home org. The service layer of `assign_role` (in `tenants/`) rejects role assignments to SA users in any org other than the SA's home org with `400 service_account_cross_org_forbidden`. Preserves the "one SA = one customer" mental model.

## v1 endpoint surface

All require `RequireHumanCaller` + the appropriate permission. Cross-org access returns 404 (not 403).

| Method | Path | Permission |
|---|---|---|
| POST | `/api/v1/security/service-accounts` | `service_accounts.manage` |
| GET | `/api/v1/security/service-accounts` | `service_accounts.read` |
| GET | `/api/v1/security/service-accounts/{sa_id}` | `service_accounts.read` |
| DELETE | `/api/v1/security/service-accounts/{sa_id}` | `service_accounts.manage` |
| POST | `/api/v1/security/service-accounts/{sa_id}/api-keys` | `service_accounts.manage` |
| GET | `/api/v1/security/service-accounts/{sa_id}/api-keys` | `service_accounts.read` |
| DELETE | `/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}` | `service_accounts.manage` |
| POST | `/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}/rotate` | `service_accounts.manage` |

`POST .../api-keys` returns the **plaintext key once** in the response body under `plaintext`; subsequent `GET` returns metadata only (`prefix`, `name`, `scopes`, timestamps). The server never sees the plaintext again.

## Audit events

| Event type | When |
|---|---|
| `service_account.created` | SA created |
| `service_account.deleted` | SA deleted (cascades keys) |
| `api_key.created` | New key minted; metadata `key_id`, `prefix` |
| `api_key.revoked` | Key revoked |
| `api_key.rotated` | Atomic create + revoke; metadata `old_key_id`, `new_key_id` |

## Last-used update — throttled

After each successful API-key auth, the verifier fires a best-effort throttled UPDATE on `api_keys.last_used_at` AND `service_accounts.last_used_at`:

```sql
UPDATE api_keys SET last_used_at = NOW()
 WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < NOW() - INTERVAL '60 seconds');
```

≤ 1 write per minute per key/SA regardless of request rate. Failure is logged and ignored. Implementation: `tokio::spawn` after the credential is verified, so it never blocks the request.

**Known limit:** under sustained high QPS on a single key, all callers still take the row lock briefly. If observed, migrate to an mpsc-channel + worker (audit-pipeline pattern). Out of scope for v1.

## Non-goals (v1)

- No per-key TTL / expiry.
- No per-key IP allowlist or rate limit (covered by [[future-enhancements/Rate-Limiting-and-Quotas]]).
- No JWT-exchange flow.
- No SA-managing-SA (humans only).
- No org-wide "list all SA keys" endpoint (per-SA list suffices).

## Related

- Spec: [`docs/superpowers/specs/2026-05-03-service-accounts-design.md`](../../docs/superpowers/specs/2026-05-03-service-accounts-design.md)
- Plan: [`docs/superpowers/plans/2026-05-03-service-accounts.md`](../../docs/superpowers/plans/2026-05-03-service-accounts.md)
- [[Authentication]] — JWT path
- [[Authorization]] — permission codes + scope intersection
- [[Architecture]] — module map
