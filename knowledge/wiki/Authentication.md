---
title: Authentication
tags:
  - auth
  - jwt
  - security
---

# Authentication

egras uses stateless JWT-based authentication with a revocation list for logout support.

## JWT Structure

Tokens are HS256-signed with a symmetric secret (`EGRAS_JWT_SECRET`, ≥32 bytes). The claims payload:

```json
{
  "sub":  "018f1234-5678-7abc-def0-123456789abc",  ← user UUID
  "org":  "018f1234-5678-7abc-def0-aabbccddeeff",  ← active organisation UUID
  "iat":  1713400000,                               ← issued at (Unix)
  "exp":  1713403600,                               ← expires at (Unix)
  "jti":  "018f1234-5678-7abc-def0-111122223333",  ← JWT ID (UUIDv7, for revocation)
  "iss":  "egras",                                  ← issuer
  "typ":  "access"                                  ← type guard
}
```

Key points:
- `org` scopes the token to one organisation. The caller's permission set is loaded for `(sub, org)`.
- `jti` is a UUIDv7 used to index the [[Data-Model#`revoked_tokens`|revoked_tokens]] table.
- `typ = "access"` guards against token type confusion (e.g., future refresh tokens).
- No leeway — tokens are rejected the instant they expire.

Encoding and decoding live in [`src/auth/jwt.rs`](../../src/auth/jwt.rs).

## Middleware

[`src/auth/middleware.rs`](../../src/auth/middleware.rs) contains `AuthLayer`, a tower `Layer` applied to all protected routes. On every request it:

### Header detection and precedence

Two header sources carry API-key credentials:

| Header | Slug in allowlist |
|--------|------------------|
| `X-API-Key: egras_<key>` | `x-api-key` |
| `Authorization: Bearer egras_<key>` | `authorization-bearer` |

**`X-API-Key` takes precedence**: if both headers are present only `X-API-Key` is read. `X-API-Key` can only carry an API key (not a JWT) — a non-`egras_*` token in that header is rejected immediately with 401.

`Authorization: Bearer` can carry either an API key (sniffed by the `egras_` prefix) or a JWT. The JWT path only applies when `Authorization: Bearer` is used.

### API-key path

1. Detect credential source (`X-API-Key` > `Authorization: Bearer`).
2. Parse the `egras_<prefix>.<secret>` token via `api_key_secret::parse`.
3. Call `ApiKeyVerifier::verify(prefix, secret)` — looks up the prefix, constant-time-compares the secret against the stored argon2 hash, checks `revoked_at`.
4. **Evaluate the per-org allowlist.** Call `FeatureEvaluator::evaluate(org_id, "auth.api_key_headers")` — see [[Feature-Flags]]. Reject with 401 if the header source's slug is not in the list.
5. Call `PermissionLoader::load(sa_user_id, org_id)`, intersect with per-key scopes.
6. Synthesise `Claims` (compatible with JWT extractors), insert `Claims`, `PermissionSet`, `Caller::ApiKey { key_id, sa_user_id, org_id }` into extensions.

Default allowlist: `["x-api-key", "authorization-bearer"]` (both accepted). Controlled per-org by the `auth.api_key_headers` flag — operator-only, `self_service = false`.

Rejection when header source is blocked: 401 `auth.unauthenticated` with `reason: "api_key_header_not_allowed:<slug>"`.

### JWT path

1. Extract `Authorization: Bearer <token>` (non-`egras_` token).
2. Decode and validate the JWT (algorithm, issuer, expiry, `typ`).
3. Call `RevocationChecker::is_revoked(jti)` — rejects with 401 if found.
4. Call `PermissionLoader::load(user_id, org_id)`.
5. Insert `Claims`, `PermissionSet`, `Caller::User { user_id, org_id, jti }` into extensions.

The allowlist flag does **not** apply to the JWT path — it governs API-key transport only.

If any step fails the request is rejected before the handler is called:

| Failure | HTTP Status |
|---------|------------|
| No credentials in any supported header | 401 `auth.unauthenticated` (`missing_credentials`) |
| `X-API-Key` carries a non-API-key token | 401 `auth.unauthenticated` (`invalid_api_key`) |
| Unknown prefix or wrong secret | 401 `auth.unauthenticated` (`invalid_api_key`) |
| Header source not in org allowlist | 401 `auth.unauthenticated` (`api_key_header_not_allowed:<slug>`) |
| Expired JWT | 401 `auth.unauthenticated` |
| Revoked JTI | 401 `auth.unauthenticated` (`token_revoked`) |
| Wrong issuer / typ | 401 `auth.unauthenticated` |

For the permission enforcement that comes *after* this, see [[Authorization]].

### Permission Loading

```sql
SELECT DISTINCT p.code
FROM user_organisation_roles uor
JOIN role_permissions rp ON rp.role_id = uor.role_id
JOIN permissions p ON p.id = rp.permission_id
WHERE uor.user_id = $1 AND uor.organisation_id = $2
```

This query is executed on every authenticated request. It is fast (indexed) and the result is immutable for the lifetime of a request.

### Revocation Checking

```sql
SELECT EXISTS(
  SELECT 1 FROM revoked_tokens
  WHERE jti = $1 AND expires_at > NOW()
)
```

Only JTIs that haven't yet expired are checked — expired entries are effectively invisible, so cleanup can be deferred.

## Auth Flows

### Login

Endpoint: `POST /api/v1/security/login`  
Service: [`src/security/service/login.rs`](../../src/security/service/login.rs)

```
1. Find user by username OR email (case-insensitive, CITEXT)
2. If not found → emit login.failed(reason: not_found) audit event → 401
3. Verify password against argon2id hash (constant-time)
4. If wrong → emit login.failed(reason: bad_password) audit event → 401
5. Fetch all org memberships for user
6. If zero memberships → 403 user.no_organisation
7. Select default org (oldest joined_at)
8. If argon2 params differ from current → opportunistically rehash + update
9. Issue JWT scoped to default org
10. Emit login.success audit event
11. Return { token, user_id, active_org_id, memberships[] }
```

> [!important] Security: no user/password distinction
> Both "user not found" and "wrong password" return 401 with the same `auth.invalid_credentials` error body. The audit event internally records the distinction, but it is never exposed to the caller. This prevents username enumeration.

### Logout

Endpoint: `POST /api/v1/security/logout`  
Service: [`src/security/service/logout.rs`](../../src/security/service/logout.rs)

```
1. Extract jti and exp from current JWT (via AuthedCaller)
2. Insert (jti, user_id, expires_at) into revoked_tokens
3. Emit logout audit event
4. Return 204 No Content
```

After logout, subsequent requests with the same token receive 401 because `RevocationChecker` finds the JTI.

### Switch Organisation

Endpoint: `POST /api/v1/security/switch-org`  
Service: [`src/security/service/switch_org.rs`](../../src/security/service/switch_org.rs)

```
1. Verify caller is a member of the requested org_id
2. If not member → 404 (hides org existence from non-members)
3. Issue new JWT scoped to requested org
4. Emit session.switched_org audit event
5. Return { token, active_org_id }
```

The old token continues to be valid until it expires — there is no forced invalidation of the previous token on switch.

### Register

Endpoint: `POST /api/v1/security/register`  
Service: [`src/security/service/register_user.rs`](../../src/security/service/register_user.rs)

This is an **invited-only** flow — not self-registration. The caller must have `users.manage_all` or `tenants.members.add` in the target org.

```
1. Validate username (1–64 chars), email (format + 254 char max), password (8–128 chars)
2. Hash password with argon2id
3. Atomically: INSERT user + INSERT user_organisation_roles in a single transaction
4. Emit user.registered audit event
5. Return 201 { user_id }
```

The atomic transaction (via `UserRepository::create_and_add_to_org`) prevents a user existing without an org membership — see [[Design-Decisions#Atomic Registration]].

### Password Reset

Two-step flow — no email delivery (stub for production). See [[Security-Domain#Password Reset]] for full detail.

### Change Password

Endpoint: `POST /api/v1/security/change-password`  
Service: [`src/security/service/change_password.rs`](../../src/security/service/change_password.rs)

```
1. Verify current password
2. Hash new password (8–128 chars)
3. Update users.password_hash
4. Emit password.changed audit event
5. Return 204
```

## Password Hashing

[`src/security/service/password_hash.rs`](../../src/security/service/password_hash.rs) wraps the `argon2` crate.

- **Algorithm:** Argon2id (hybrid side-channel + GPU resistance)
- **Parameters:** `m=19456` KiB, `t=2` iterations, `p=1` parallelism
- **Output:** PHC string format — `$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`
- **Salt:** 16 random bytes from `OsRng` per hash
- **Timing:** ~100ms on a modern CPU — acceptable for interactive login, prevents brute-force

**Opportunistic rehash:** On every successful login, `needs_rehash()` checks whether the stored hash uses current parameters. If not (e.g., parameters were upgraded), the new hash is computed and saved — without requiring the user to do anything. This means parameter upgrades roll out organically as users log in.

## Related notes

- [[Authorization]] — what happens after auth: permission extraction and enforcement
- [[Security-Domain]] — full security domain: all use cases and types
- [[Data-Model#`revoked_tokens`]] — the revocation table schema
- [[Design-Decisions]] — why JWT + revocation list instead of sessions
