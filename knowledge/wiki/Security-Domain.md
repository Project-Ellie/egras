---
title: Security Domain
tags:
  - security
  - users
  - passwords
---

# Security Domain

The security domain (`src/security/`) owns everything related to users, authentication, and session management. It contains the most use cases of any domain.

See [[Authentication]] for the auth protocol details and [[Authorization]] for permission enforcement.

## Domain Types

Defined in [`src/security/model.rs`](../../src/security/model.rs):

### User

```rust
pub struct User {
    pub id:            Uuid,
    pub username:      String,
    pub email:         String,      // stored as citext in DB
    pub password_hash: String,      // Argon2id PHC format
    pub created_at:    DateTime<Utc>,
    pub updated_at:    DateTime<Utc>,
}
```

### UserMembership

Returned in login responses to describe a user's org context:

```rust
pub struct UserMembership {
    pub org_id:     Uuid,
    pub org_name:   String,
    pub role_codes: Vec<String>,
    pub joined_at:  DateTime<Utc>,
}
```

### PasswordResetToken

```rust
pub struct PasswordResetToken {
    pub id:           Uuid,
    pub token_hash:   String,          // SHA-256 hex — raw token never stored
    pub user_id:      Uuid,
    pub expires_at:   DateTime<Utc>,
    pub consumed_at:  Option<DateTime<Utc>>,
    pub created_at:   DateTime<Utc>,
}
```

### UserCursor

Opaque cursor for paginated user listing:

```rust
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub id:         Uuid,
}
```

See [[Pagination]] for how cursors work.

## Use Cases

Each use case lives in its own file under `src/security/service/`.

### Login

File: [`src/security/service/login.rs`](../../src/security/service/login.rs)  
Endpoint: `POST /api/v1/security/login`  
Auth: none (public)

Full flow described in [[Authentication#Login]].

**Error types:**
- `LoginError::InvalidCredentials` → 401
- `LoginError::UserHasNoOrganisation` → 403
- `LoginError::Internal` → 500

### Register User

File: [`src/security/service/register_user.rs`](../../src/security/service/register_user.rs)  
Endpoint: `POST /api/v1/security/register`  
Auth: `users.manage_all` OR `tenants.members.add`

Validates:
- Username: 1–64 chars, trimmed
- Email: non-empty local part, domain with interior dot, ≤254 chars total
- Password: 8–128 chars

Then atomically creates the user and org membership via `UserRepository::create_and_add_to_org`, which wraps both inserts in a single PostgreSQL transaction.

**Error types:**
- `RegisterUserError::InvalidUsername` → 400 field error `username.invalid`
- `RegisterUserError::InvalidEmail` → 400 field error `email.invalid`
- `RegisterUserError::PasswordTooShort` → 400 field error `password.too_short`
- `RegisterUserError::PasswordTooLong` → 400 field error `password.too_long`
- `RegisterUserError::DuplicateUsername` → 409
- `RegisterUserError::DuplicateEmail` → 409
- `RegisterUserError::OrgNotFound` → 404
- `RegisterUserError::UnknownRoleCode` → 400

### Logout

File: [`src/security/service/logout.rs`](../../src/security/service/logout.rs)  
Endpoint: `POST /api/v1/security/logout`  
Auth: required

Inserts the current JWT's `jti` into `revoked_tokens`. Subsequent requests with the same token receive 401. See [[Authentication#Logout]].

### Switch Organisation

File: [`src/security/service/switch_org.rs`](../../src/security/service/switch_org.rs)  
Endpoint: `POST /api/v1/security/switch-org`  
Auth: required

Issues a new JWT scoped to the requested organisation. The caller must be a member of that org. See [[Authentication#Switch Organisation]].

**Error types:**
- `SwitchOrgError::NotMember` → 404 (hides org existence)

### Change Password

File: [`src/security/service/change_password.rs`](../../src/security/service/change_password.rs)  
Endpoint: `POST /api/v1/security/change-password`  
Auth: required (any authenticated user)

**Error types:**
- `ChangePasswordError::WrongCurrentPassword` → 400 field error
- `ChangePasswordError::PasswordTooShort` / `PasswordTooLong` → 400 field errors

### Password Reset

Two-step, no email delivery (email stub).

#### Step 1 — Request Reset Token

File: [`src/security/service/password_reset_request.rs`](../../src/security/service/password_reset_request.rs)  
Endpoint: `POST /api/v1/security/password-reset-request`  
Auth: none (public)

```
1. Validate email format
2. Look up user by email
3. If user not found → return success anyway (no user enumeration)
4. Check if user already has pending unexpired tokens (safety limit)
5. Generate 32 random bytes → raw token
6. Hash with SHA-256 → token_hash
7. Insert (token_hash, user_id, expires_at) into password_reset_tokens
8. Log raw token at INFO level (production would send email instead)
9. Emit password.reset_requested audit event
10. Return 204 (always, regardless of whether user was found)
```

#### Step 2 — Confirm Reset

File: [`src/security/service/password_reset_confirm.rs`](../../src/security/service/password_reset_confirm.rs)  
Endpoint: `POST /api/v1/security/password-reset-confirm`  
Auth: none (public)

```
1. Hash submitted token with SHA-256
2. Look up token_hash in password_reset_tokens
3. Verify not expired, not already consumed
4. Validate new password (8–128 chars)
5. Mark token as consumed (consumed_at = NOW())
6. Hash new password with argon2id
7. Update users.password_hash
8. Emit password.reset_confirmed(success) audit event
9. Return 204
```

**Error types:**
- `PasswordResetConfirmError::InvalidOrExpiredToken` → 400
- `PasswordResetConfirmError::PasswordTooShort/TooLong` → 400

### List Users

File: [`src/security/service/list_users.rs`](../../src/security/service/list_users.rs)  
Endpoint: `GET /api/v1/users`  
Auth: `users.manage_all`

Returns a paginated list of users platform-wide, with their org memberships per user. Uses cursor-based pagination — see [[Pagination]].

Optional query params:
- `q` — username/email search (prefix match)
- `org_id` — filter to users in a specific org
- `after` — cursor
- `limit` — page size (default 20)

### Bootstrap Seed Admin

File: [`src/security/service/bootstrap_seed_admin.rs`](../../src/security/service/bootstrap_seed_admin.rs)  
CLI only: `egras seed-admin`

Not accessible via HTTP. Synchronously creates the first operator admin user. See [[Configuration#Bootstrap: seed-admin]].

## Repositories

### UserRepository

Trait: [`src/security/persistence/user_repository.rs`](../../src/security/persistence/user_repository.rs)  
Impl: [`src/security/persistence/user_repository_pg.rs`](../../src/security/persistence/user_repository_pg.rs)

Key methods:

| Method | Description |
|--------|-------------|
| `create(username, email, hash)` | Insert user |
| `find_by_username_or_email(s)` | Case-insensitive lookup (CITEXT) |
| `find_by_id(id)` | By UUID |
| `update_password_hash(id, hash)` | After password change/reset |
| `list_memberships(user_id)` | All org memberships for a user |
| `list_users(org_id?, q?, cursor?, limit)` | Paginated platform user listing |
| `list_memberships_for_users(ids)` | Batch membership lookup |
| `create_and_add_to_org(...)` | Atomic: create user + add to org in one transaction |

The `create_and_add_to_org` method is particularly important — see [[Design-Decisions#Atomic Registration]].

### TokenRepository

Trait: [`src/security/persistence/token_repository.rs`](../../src/security/persistence/token_repository.rs)  
Impl: [`src/security/persistence/token_repository_pg.rs`](../../src/security/persistence/token_repository_pg.rs)

Handles both password-reset tokens and JWT revocation:

| Method | Purpose |
|--------|---------|
| `insert(user_id, hash, expires_at)` | Create new reset token |
| `find_valid(hash)` | Find non-expired, non-consumed token |
| `consume(token_id)` | Mark token as used |
| `count_pending_for_user(user_id)` | Safety check (prevent token spam) |
| `revoke(jti, user_id, expires_at)` | JWT logout revocation |
| `is_revoked(jti)` | Check by JWT middleware |

## HTTP Interface

[`src/security/interface.rs`](../../src/security/interface.rs) registers two groups of routes:

**Public router** (no auth):
- `POST /api/v1/security/login`
- `POST /api/v1/security/register`
- `POST /api/v1/security/password-reset-request`
- `POST /api/v1/security/password-reset-confirm`

**Protected router** (requires valid JWT):
- `POST /api/v1/security/logout`
- `POST /api/v1/security/switch-org`
- `POST /api/v1/security/change-password`

**Top-level protected** (wired in `lib.rs`):
- `GET /api/v1/users` (requires `users.manage_all`)

## Related notes

- [[Authentication]] — JWT flows and middleware detail
- [[Authorization]] — permission enforcement
- [[Data-Model]] — `users`, `password_reset_tokens`, `revoked_tokens` tables
- [[Audit-System]] — all security audit events
- [[Testing-Strategy]] — how security tests are organised
