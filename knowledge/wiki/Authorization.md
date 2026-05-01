---
title: Authorization
tags:
  - rbac
  - permissions
  - auth
---

# Authorization

egras uses role-based access control (RBAC). Permissions are stored in the database, loaded per request by the [[Authentication#Middleware|auth middleware]], and enforced via type-safe Axum extractors.

## Concepts

| Term | Meaning |
|------|---------|
| **Permission** | An atomic code like `tenants.create`. Stored in the `permissions` table. |
| **Role** | A named collection of permissions (e.g., `org_admin`). Stored in `roles`. |
| **Membership** | A `(user, organisation, role)` triple in `user_organisation_roles`. |
| **PermissionSet** | The set of permission codes loaded for a caller's `(user_id, org_id)` pair on each request. |

A user's effective permissions are the union of all permissions from all roles they hold in their currently-active organisation (the one in the JWT `org` claim).

## PermissionSet

[`src/auth/permissions.rs`](../../src/auth/permissions.rs) defines `PermissionSet`:

```rust
pub struct PermissionSet {
    codes: HashSet<String>,
}

impl PermissionSet {
    pub fn has(&self, code: &str) -> bool
    pub fn has_any(&self, codes: &[&str]) -> bool
    pub fn is_operator_over_tenants(&self) -> bool  // has "tenants.manage_all"
    pub fn is_operator_over_users(&self) -> bool    // has "users.manage_all"
    pub fn is_audit_read_all(&self) -> bool         // has "audit.read_all"
}
```

`PermissionSet` is injected into request extensions by `AuthLayer` and extracted by handlers via `AuthedCaller` or `Perm<P>`.

## Extractors

Two patterns for enforcing permissions in handlers, both defined in [`src/auth/extractors.rs`](../../src/auth/extractors.rs).

### Pattern 1 — Type-level extractor (preferred)

```rust
pub struct Perm<P: Permission>(PhantomData<P>);

impl<S, P: Permission> FromRequestParts<S> for Perm<P> {
    async fn from_request_parts(parts: &mut Parts, _: &S)
        -> Result<Self, AppError>
    {
        let perms = parts.extensions.get::<PermissionSet>()...;
        if P::accepts(perms) {
            Ok(Perm(PhantomData))
        } else {
            Err(AppError::PermissionDenied { code: P::CODE })
        }
    }
}
```

Usage in a handler:

```rust
async fn create_organisation(
    caller: AuthedCaller,
    _perm: Perm<TenantsCreate>,   // ← rejected with 403 before body runs
    State(state): State<AppState>,
    Json(body): Json<CreateOrgRequest>,
) -> Result<impl IntoResponse, AppError> { ... }
```

The permission check happens at the Axum extractor level, before the handler body runs. If the caller lacks the required permission, Axum returns 403 immediately.

### Pattern 2 — Runtime check

For cases where the required permission depends on runtime data (e.g., which org is being accessed):

```rust
use crate::auth::permissions::{require_permission, authorise_org};

async fn some_handler(
    caller: AuthedCaller,
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    require_permission(&caller.permissions, "tenants.members.add")?;
    authorise_org(&caller.claims, org_id, &caller.permissions)?;
    ...
}
```

### Permission marker traits

Each permission is a zero-sized type implementing the `Permission` trait:

```rust
pub trait Permission {
    const CODE: &'static str;
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE)  // default: exact match
    }
}

pub struct TenantsCreate;
impl Permission for TenantsCreate {
    const CODE: &'static str = "tenants.create";
}

pub struct TenantsMembersList;
impl Permission for TenantsMembersList {
    const CODE: &'static str = "tenants.members.list";
    fn accepts(set: &PermissionSet) -> bool {
        // operator_admin bypass: tenants.manage_all also grants this
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}
```

The `accepts()` override allows operator bypass logic to live alongside the permission definition, not scattered through handler code.

## Org Scoping

When a caller accesses resources belonging to a specific organisation, `authorise_org` enforces cross-tenant rules:

```
Is the target org_id == caller's JWT org_id?
  YES → allow
  NO  →
    Does caller have tenants.manage_all?  → allow (operator bypass)
    Does caller have audit.read_all?      → allow (for audit queries)
    Otherwise                             → 404 Not Found
```

> [!important] Why 404, not 403?
> Returning 403 on cross-org access would confirm that the organisation exists. Returning 404 hides the existence of other tenants from unauthorised callers. This is a deliberate security choice — see [[Design-Decisions#404 vs 403 for cross-org access]].

## Operator Organisation

The operator organisation (seeded in migration 0005 with ID `00000000-0000-0000-0000-000000000001`) is a special tenant. Users with `operator_admin` role in this org hold `tenants.manage_all`, `users.manage_all`, and `audit.read_all`.

These permissions don't grant access via an explicit bypass in every handler — instead, the `PermissionSet` helper methods (`is_operator_over_tenants()`, etc.) abstract the check, and permission marker types' `accepts()` methods use them where appropriate.

## All Permission Codes and Their Handlers

| Code | Handler(s) |
|------|-----------|
| `tenants.create` | `POST /api/v1/tenants/organisations` |
| `tenants.update` | `PATCH /api/v1/tenants/organisations/{id}` |
| `tenants.read` | `GET /api/v1/tenants/organisations/{id}` |
| `tenants.members.add` | `POST /api/v1/tenants/organisations/{id}/members` (also allows registration) |
| `tenants.members.remove` | `DELETE /api/v1/tenants/organisations/{id}/members/{uid}` |
| `tenants.members.list` | `GET /api/v1/tenants/organisations/{id}/members` |
| `tenants.roles.assign` | `POST /api/v1/tenants/organisations/{id}/members/{uid}/roles` |
| `users.manage_all` | `POST /api/v1/security/register` (cross-tenant registration) |
| `audit.read_all` | `GET /api/v1/audit/events` (any org) |
| `audit.read_own_org` | `GET /api/v1/audit/events` (own org only) |
| `tenants.manage_all` | Operator bypass — implicitly grants `tenants.*` on any org |

## AuthedCaller Extractor

`AuthedCaller` is a convenience extractor that bundles both `Claims` and `PermissionSet`:

```rust
pub struct AuthedCaller {
    pub claims: Claims,
    pub permissions: PermissionSet,
}
```

It fails with 401 if the JWT is absent or invalid. Handlers that need the caller identity but don't have a fixed required permission use `AuthedCaller` alone, then call runtime checks as needed.

## Related notes

- [[Authentication]] — how the JWT is validated and permissions are loaded
- [[Data-Model#Permission matrix]] — which roles have which permissions
- [[Developer-Guide#Add a new permission]] — step-by-step for adding permissions
- [[Design-Decisions]] — rationale for 404 vs 403 on cross-org access
