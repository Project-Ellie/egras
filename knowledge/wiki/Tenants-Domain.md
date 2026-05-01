---
title: Tenants Domain
tags:
  - tenants
  - organisations
  - rbac
---

# Tenants Domain

The tenants domain (`src/tenants/`) manages organisations (tenants), their membership, and role assignment. It is the RBAC backbone of the platform.

## Domain Types

Defined in [`src/tenants/model.rs`](../../src/tenants/model.rs):

### Organisation

```rust
pub struct Organisation {
    pub id:          Uuid,
    pub name:        String,
    pub business:    String,
    pub is_operator: bool,
    pub created_at:  DateTime<Utc>,
    pub updated_at:  DateTime<Utc>,
}
```

### OrganisationSummary

Used in list responses — includes the caller's role in that org:

```rust
pub struct OrganisationSummary {
    pub id:         Uuid,
    pub name:       String,
    pub business:   String,
    pub role_codes: Vec<String>,   // caller's roles in this org
    pub created_at: DateTime<Utc>,
}
```

### Role

```rust
pub struct Role {
    pub id:   Uuid,
    pub code: String,  // e.g., "org_admin"
}
```

### Membership

```rust
pub struct Membership {
    pub user_id:         Uuid,
    pub organisation_id: Uuid,
    pub role_id:         Uuid,
    pub role_code:       String,
    pub created_at:      DateTime<Utc>,
}
```

### MemberSummary

Used in member list responses:

```rust
pub struct MemberSummary {
    pub user_id:    Uuid,
    pub username:   String,
    pub email:      String,
    pub role_codes: Vec<String>,   // all roles in this org
    pub joined_at:  DateTime<Utc>, // MIN(created_at) of their role rows
}
```

## Use Cases

### Create Organisation

File: [`src/tenants/service/create_organisation.rs`](../../src/tenants/service/create_organisation.rs)  
Endpoint: `POST /api/v1/tenants/organisations`  
Auth: `tenants.create`

Creates a new organisation and atomically adds the calling user as its first `org_owner`. Uses `OrganisationRepository::create_with_initial_owner`, a single transaction.

**Error types:**
- `CreateOrganisationError::DuplicateName` → 409
- `CreateOrganisationError::UnknownRoleCode` → 400

### Add User to Organisation

File: [`src/tenants/service/add_user_to_organisation.rs`](../../src/tenants/service/add_user_to_organisation.rs)  
Endpoint: `POST /api/v1/tenants/organisations/{org_id}/members`  
Auth: `tenants.members.add` (or `tenants.manage_all` operator bypass)

Adds an existing user to an organisation with a specified role. The operation is idempotent — if the user already has that role in the org, it succeeds silently.

**Org scoping:** If the caller's `org` claim doesn't match `org_id`, they need `tenants.manage_all` — otherwise 404.

**Error types:**
- `AddUserError::UserNotFound` → 404
- `AddUserError::OrgNotFound` → 404
- `AddUserError::UnknownRoleCode` → 400

### Remove User from Organisation

File: [`src/tenants/service/remove_user_from_organisation.rs`](../../src/tenants/service/remove_user_from_organisation.rs)  
Endpoint: `DELETE /api/v1/tenants/organisations/{org_id}/members/{user_id}`  
Auth: `tenants.members.remove`

Removes all of a user's roles in the given org. Fails if the user is the last `org_owner` (at least one owner must remain).

**Error types:**
- `RemoveUserError::UserNotFound` → 404
- `RemoveUserError::NotMember` → 404
- `RemoveUserError::LastOwner` → 409

### List My Organisations

File: [`src/tenants/service/list_my_organisations.rs`](../../src/tenants/service/list_my_organisations.rs)  
Endpoint: `GET /api/v1/tenants/my-organisations`  
Auth: required (any authenticated user)

Returns a paginated list of organisations the calling user belongs to, including their role(s) in each. Uses cursor-based pagination — see [[Pagination]].

### List Organisation Members

File: [`src/tenants/service/list_organisation_members.rs`](../../src/tenants/service/list_organisation_members.rs)  
Endpoint: `GET /api/v1/tenants/organisations/{org_id}/members`  
Auth: `tenants.members.list`

Returns a paginated list of all members in the given org, each with their role codes and `joined_at` timestamp.

**Org scoping:** Non-members (without `tenants.manage_all`) get 404 when querying a foreign org.

### Assign Role

File: [`src/tenants/service/assign_role.rs`](../../src/tenants/service/assign_role.rs)  
Endpoint: `POST /api/v1/tenants/organisations/{org_id}/members/{user_id}/roles`  
Auth: `tenants.roles.assign`

Assigns an additional role to a user who is already a member of the org. Idempotent — assigning a role the user already holds succeeds silently.

**Error types:**
- `AssignRoleError::UserNotFound` → 404
- `AssignRoleError::NotMember` → 404
- `AssignRoleError::UnknownRoleCode` → 400

## Repositories

### OrganisationRepository

Trait: [`src/tenants/persistence/organisation_repository.rs`](../../src/tenants/persistence/organisation_repository.rs)  
Impl: [`src/tenants/persistence/organisation_repository_pg.rs`](../../src/tenants/persistence/organisation_repository_pg.rs)

Key methods:

| Method | Description |
|--------|-------------|
| `create(name, business)` | Insert organisation |
| `create_with_initial_owner(name, business, user_id, role_code)` | Atomic: create org + add user as owner |
| `list_for_user(user_id, after?, limit)` | Orgs the user belongs to (paginated) |
| `list_members(org_id, after?, limit)` | Members of an org (paginated) |
| `is_member(user_id, org_id)` | Membership check |
| `add_member(user_id, org_id, role_code)` | Idempotent: add role to user in org |
| `remove_member_checked(user_id, org_id)` | Remove all roles; fails if last owner |
| `find_by_name(name)` | Used by `seed-admin` CLI |

### RoleRepository

Trait: [`src/tenants/persistence/role_repository.rs`](../../src/tenants/persistence/role_repository.rs)  
Impl: [`src/tenants/persistence/role_repository_pg.rs`](../../src/tenants/persistence/role_repository_pg.rs)

| Method | Description |
|--------|-------------|
| `find_by_code(code)` | Look up a role by its code string |
| `assign(user_id, org_id, role_id)` | Idempotent role assignment |
| `has_role(user_id, org_id, role_id)` | Membership + role check |

## HTTP Interface

[`src/tenants/interface.rs`](../../src/tenants/interface.rs) registers all routes under the protected router:

| Method | Path | Permission |
|--------|------|-----------|
| `POST` | `/api/v1/tenants/organisations` | `tenants.create` |
| `GET` | `/api/v1/tenants/my-organisations` | any authenticated user |
| `GET` | `/api/v1/tenants/organisations/{id}/members` | `tenants.members.list` |
| `POST` | `/api/v1/tenants/organisations/{id}/members` | `tenants.members.add` |
| `DELETE` | `/api/v1/tenants/organisations/{id}/members/{uid}` | `tenants.members.remove` |
| `POST` | `/api/v1/tenants/organisations/{id}/members/{uid}/roles` | `tenants.roles.assign` |

## Multi-role Support

A user can hold **multiple roles** in the same organisation. For example, a user might be both `org_owner` and `org_admin`. Their effective `PermissionSet` is the union of all permissions from all roles they hold. `MemberSummary.role_codes` is a `Vec<String>` for this reason.

## Operator Bypass

Users in the operator org with `tenants.manage_all` can perform all tenant operations on any organisation — they are not restricted to their JWT's `org` claim. This bypass is encoded in the `accepts()` method of each permission marker type. See [[Authorization#Operator Organisation]].

## Related notes

- [[Authorization]] — permission codes and org-scoping rules
- [[Data-Model]] — `organisations`, `roles`, `user_organisation_roles` tables
- [[Audit-System]] — tenants audit events
- [[Testing-Strategy]] — how tenants tests are organised
