# Design: List All Users

**Date:** 2026-04-30  
**Status:** Approved  
**Jira:** KAN-1

---

## 1. Overview

Add `GET /api/v1/users` — a paginated, filterable endpoint that returns platform users enriched with their full org memberships. Operators see all platform users; tenant admins see only users within their own org.

---

## 2. API Contract

### Endpoint

```
GET /api/v1/users
```

### Query Parameters

| Parameter | Type    | Required | Description                                                    |
|-----------|---------|----------|----------------------------------------------------------------|
| `after`   | string  | no       | Opaque cursor from previous response's `next_cursor`           |
| `limit`   | integer | no       | Page size, 1–100, default 20                                   |
| `org_id`  | UUID    | no       | Filter to users belonging to this org                          |
| `q`       | string  | no       | Case-insensitive contains match (`ILIKE '%q%'`) on `username` or `email` |

### Authorization

- `users.manage_all` → **operator path**: all platform users, all their org memberships
- `tenants.members.list` (+ caller must be a member of the target org) → **tenant admin path**: users in caller's org only, memberships scoped to that org

### Response — 200 OK

```json
{
  "items": [
    {
      "id": "018f1a2b-...",
      "username": "alice",
      "email": "alice@example.com",
      "created_at": "2026-01-15T10:00:00Z",
      "memberships": [
        {
          "org_id": "018f1a2c-...",
          "org_name": "Acme Corp",
          "role_codes": ["owner"],
          "joined_at": "2026-01-15T10:01:00Z"
        }
      ]
    }
  ],
  "next_cursor": "eyJjcmVhdGVkX2F0IjoiMjAyNi0wMS0xNVQxMDowMDowMFoiLCJ1c2VyX2lkIjoiMDE4ZjFhMmItLi4uIn0="
}
```

`next_cursor` is `null` when no further pages exist.

### Error Responses (RFC 7807)

| Status | Slug                  | Condition                         |
|--------|-----------------------|-----------------------------------|
| 401    | `unauthenticated`     | Missing or invalid JWT            |
| 403    | `permission_denied`   | Caller lacks required permission  |
| 400    | `invalid_cursor`      | Cursor cannot be decoded          |
| 400    | `invalid_limit`       | Limit out of 1–100 range          |

---

## 3. Architecture

### Domain placement

`GET /api/v1/users` lives at the top level (not under `/tenants` or `/security`), but the implementation code lives in the `security` domain — users are a security concept.

### Layers

```
src/security/interface.rs          ← new route + handler + DTOs
src/security/service/list_users.rs ← new use-case file
src/security/persistence/
  user_repository.rs               ← two new trait methods
  user_repository_pg.rs            ← PostgreSQL implementations
```

### Data Flow

1. Handler extracts `Claims` + `PermissionSet` from request extensions.
2. Handler checks `users.manage_all` OR `tenants.members.list`; builds `ListUsersInput`.
3. Service decodes cursor → calls `UserRepository::list_users(org_id, q, cursor, limit+1)`.
4. Service calls `UserRepository::list_memberships_for_users(&user_ids)` — single batch query.
5. Service groups memberships by `user_id`; for non-operators, filters memberships to `caller_org_id` only.
6. Service detects next page (over-fetch pattern), encodes `UserCursor`, returns `ListUsersOutput`.
7. Handler serialises to `ListUsersResponse` → 200.

### New Repository Methods

```rust
async fn list_users(
    &self,
    org_id: Option<Uuid>,
    q: Option<&str>,
    cursor: Option<UserCursor>,
    limit: u32,
) -> Result<Vec<User>, UserRepoError>;

async fn list_memberships_for_users(
    &self,
    user_ids: &[Uuid],
) -> Result<Vec<UserMembership>, UserRepoError>;
```

### Cursor Type

```rust
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
```

Tie-breaking on `(created_at, user_id)` ensures stable ordering across pages. Encoded/decoded via the existing `cursor_codec` module.

### New Permission Marker

```rust
pub struct UsersRead;
impl Permission for UsersRead {
    const CODE: &'static str = "users.read";
    fn accepts(set: &PermissionSet) -> bool {
        set.has("users.read") || set.is_operator_over_users()
    }
}
```

Tenant admins hold `users.read` scoped to their org; operators hold `users.manage_all` which bypasses via `is_operator_over_users()`.

### Audit

Emit `AuditEvent { action: "users.list", actor_id, org_id }` via `AppState::audit_recorder()` on every successful response.

---

## 4. Testing Strategy

### Persistence (`tests/security_persistence_test.rs` — extended)

- `list_users_returns_all_platform_users` — operator path, no filters
- `list_users_filtered_by_org_id` — `org_id` filter returns only members of that org
- `list_users_search_by_username` — `q` matches on username
- `list_users_search_by_email` — `q` matches on email
- `list_users_cursor_pagination` — verifies page boundaries and cursor continuity
- `list_memberships_for_users_batch` — returns correct memberships for a set of user IDs

### Service (`tests/security_service_list_users_test.rs` — new)

- `operator_sees_all_users_with_all_memberships`
- `tenant_admin_sees_only_org_users_memberships_scoped_to_org`
- `invalid_cursor_returns_error`
- `limit_clamped_to_100`

### Interface (`tests/security_http_list_users_test.rs` — new)

- `unauthenticated_returns_401`
- `insufficient_permission_returns_403`
- `operator_list_returns_full_memberships`
- `tenant_admin_list_scoped_to_own_org`
- `pagination_next_cursor_present_when_more_results`
- `filter_by_org_id_works`
- `search_q_filters_results`

---

## 5. Out of Scope

- `GET /api/v1/users/:id` (single user detail) — deferred
- Export / bulk download — deferred
- Sorting options beyond default `(created_at ASC, id ASC)` — deferred
