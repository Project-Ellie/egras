---
title: Pagination
tags:
  - pagination
  - cursor
  - api
---

# Pagination

egras uses cursor-based pagination throughout — not offset/limit. Cursors are **opaque** to API consumers: they must be treated as black boxes and passed back verbatim.

The cursor codec lives in [`src/pagination.rs`](../../src/pagination.rs).

## Why cursor pagination?

| | Cursor | Offset |
|--|--------|--------|
| **Stable pages** | Yes — inserting rows doesn't shift pages | No — new rows push items to the next page |
| **Scalable** | Yes — no `COUNT(*)` or `OFFSET n` needed | No — `OFFSET` scans grow with page number |
| **Random access** | No — must walk forward | Yes |
| **Stateless** | Yes — cursor encodes all context | Yes |

For audit logs and user lists that are append-only or rarely change, cursors are the natural fit.

## Cursor Format

A cursor is `base64url(json(T))` where `T` is a per-endpoint struct containing enough context to resume from the correct position.

For most endpoints, `T` is a `(timestamp, id)` pair:

```rust
// src/security/model.rs
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub id:         Uuid,
}

// src/tenants/model.rs
pub struct OrganisationCursor {
    pub created_at: DateTime<Utc>,
    pub id:         Uuid,
}

pub struct MembershipCursor {
    pub joined_at: DateTime<Utc>,
    pub id:        Uuid,    // user_id
}

// Audit events use:
pub struct AuditCursor {
    pub occurred_at: DateTime<Utc>,
    pub id:          Uuid,
}
```

The composite `(timestamp, id)` key is stable even when multiple rows share the same timestamp (UUID v7 is time-ordered but collisions are theoretically possible at high write rates — the `id` acts as a tiebreaker).

## The Codec

```rust
// src/pagination.rs

pub fn encode<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_vec(value).expect("cursor must serialize");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

pub fn decode<T: DeserializeOwned>(raw: &str) -> Result<T, CursorDecodeError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| CursorDecodeError)?;
    serde_json::from_slice::<T>(&bytes).map_err(|_| CursorDecodeError)
}
```

URL-safe base64 (no padding) is used so cursors are safe to include in query strings without percent-encoding.

> [!warning] Cursors are NOT stable across versions
> The encoding is `base64url(json(T))` — if the cursor struct changes (field added/removed/renamed), old cursors will fail to decode. Clients must not store cursors across deployments. This is documented in the source and the API spec.

## How Pagination Works in a Service

Pattern used in every paginated service function:

```rust
// Decode incoming cursor (if any)
let cursor = match input.after {
    Some(raw) => Some(
        cursor_codec::decode::<UserCursor>(&raw)
            .map_err(|_| ListUsersError::InvalidCursor)?
    ),
    None => None,
};

// Build query with cursor condition
let rows = state.users.list_users(
    input.org_id,
    input.q.as_deref(),
    cursor.as_ref(),
    input.limit + 1,  // fetch one extra to detect if there's a next page
).await?;

// Determine if there's a next page
let has_next = rows.len() > input.limit;
let rows = if has_next { &rows[..input.limit] } else { &rows[..] };

// Encode next cursor from last row
let next_cursor = if has_next {
    let last = rows.last().unwrap();
    Some(cursor_codec::encode(&UserCursor {
        created_at: last.created_at,
        id: last.id,
    }))
} else {
    None
};
```

The "fetch N+1" trick avoids a separate `COUNT` query — if `N+1` rows come back, there's another page.

## How Pagination Works in a Repository

The SQL uses `(timestamp, id) > (cursor_timestamp, cursor_id)` as a keyset filter:

```sql
-- list_users: after cursor
SELECT u.id, u.username, u.email, u.created_at
FROM users u
WHERE
  ($1::timestamptz IS NULL OR u.created_at > $1)
  OR (u.created_at = $1 AND u.id > $2::uuid)
ORDER BY u.created_at ASC, u.id ASC
LIMIT $3
```

This is more efficient than `OFFSET` because the database can use a B-tree index on `(created_at, id)`.

## API Contract

Every paginated endpoint returns:

```json
{
  "items": [ ... ],
  "next_cursor": "eyJjcmVhdGVkX2F0IjoiMjAyNi0wMS0wMVQwMDowMDowMFoiLCJpZCI6Ii4uLiJ9"
}
```

- `next_cursor` is `null` when there are no more results
- Pass `?after=<cursor>` to fetch the next page
- `limit` defaults and maximums are endpoint-specific (typically 10–100)

## Endpoints with Pagination

| Endpoint | Cursor struct | Sort key |
|----------|--------------|----------|
| `GET /api/v1/users` | `UserCursor` | `(created_at, id)` |
| `GET /api/v1/tenants/my-organisations` | `OrganisationCursor` | `(created_at, id)` |
| `GET /api/v1/tenants/organisations/{id}/members` | `MembershipCursor` | `(joined_at, user_id)` |
| `GET /api/v1/audit/events` | `AuditCursor` | `(occurred_at, id)` |

## Related notes

- [[Architecture]] — where `src/pagination.rs` sits
- [[Audit-System#Querying Audit Events]] — audit pagination in practice
- [[Security-Domain#List Users]] — user listing
- [[Tenants-Domain#List My Organisations]] — org listing
