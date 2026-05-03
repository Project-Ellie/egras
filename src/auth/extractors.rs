use std::marker::PhantomData;

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::auth::jwt::Claims;
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

/// Indicates which kind of credential authenticated the current request.
/// Inserted into request extensions by `AuthLayer` alongside `Claims` and
/// `PermissionSet`. Handlers that need to differentiate (or reject API keys)
/// extract this; everything else continues to extract `AuthedCaller`.
#[derive(Debug, Clone)]
pub enum Caller {
    User {
        user_id: Uuid,
        org_id: Uuid,
        jti: Uuid,
    },
    ApiKey {
        key_id: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
    },
}

impl Caller {
    pub fn org_id(&self) -> Uuid {
        match self {
            Caller::User { org_id, .. } | Caller::ApiKey { org_id, .. } => *org_id,
        }
    }
    pub fn principal_user_id(&self) -> Uuid {
        match self {
            Caller::User { user_id, .. } => *user_id,
            Caller::ApiKey { sa_user_id, .. } => *sa_user_id,
        }
    }
    pub fn is_user(&self) -> bool {
        matches!(self, Caller::User { .. })
    }
}

/// Extractor that succeeds only when the caller is a human user (i.e. JWT auth).
/// Returns 403 `auth.requires_user_credentials` for API-key callers and 401 if
/// the layer is misconfigured (no `Caller` in extensions).
pub struct RequireHumanCaller;

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireHumanCaller {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let caller =
            parts
                .extensions
                .get::<Caller>()
                .cloned()
                .ok_or_else(|| AppError::Unauthenticated {
                    reason: "no_caller".into(),
                })?;
        match caller {
            Caller::User { .. } => Ok(RequireHumanCaller),
            Caller::ApiKey { .. } => Err(AppError::RequiresUserCredentials),
        }
    }
}

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Caller>()
            .cloned()
            .ok_or_else(|| AppError::Unauthenticated {
                reason: "no_caller".into(),
            })
    }
}

/// Extractor that copies `Claims` and `PermissionSet` out of the request
/// extensions (inserted by `AuthLayer`). Returns `Unauthenticated` if the
/// layer is misconfigured; in the normal case the layer short-circuits on
/// missing/invalid bearer tokens before handlers run.
#[derive(Debug, Clone)]
pub struct AuthedCaller {
    pub claims: Claims,
    pub permissions: PermissionSet,
}

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthedCaller {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims =
            parts
                .extensions
                .get::<Claims>()
                .cloned()
                .ok_or_else(|| AppError::Unauthenticated {
                    reason: "no_claims".into(),
                })?;
        let permissions = parts
            .extensions
            .get::<PermissionSet>()
            .cloned()
            .ok_or_else(|| AppError::Unauthenticated {
                reason: "no_permission_set".into(),
            })?;
        Ok(Self {
            claims,
            permissions,
        })
    }
}

/// Marker trait — one zero-sized type per permission code.
pub trait Permission {
    const CODE: &'static str;
    /// Whether the provided permission set grants this permission.
    /// Default: direct `has(CODE)`. Override for permissions with operator bypass.
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE)
    }
}

/// Zero-sized extractor that enforces `P::accepts()` on the caller. Runs as a
/// `FromRequestParts` extractor so it evaluates BEFORE the `Json<Body>`
/// extractor, preserving the 401 > 403 > 400 > 2xx precedence.
pub struct Perm<P: Permission>(PhantomData<P>);

#[async_trait]
impl<S: Send + Sync, P: Permission + 'static> FromRequestParts<S> for Perm<P> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let set =
            parts
                .extensions
                .get::<PermissionSet>()
                .ok_or_else(|| AppError::Unauthenticated {
                    reason: "no_permission_set".into(),
                })?;
        if P::accepts(set) {
            Ok(Perm(PhantomData))
        } else {
            Err(AppError::PermissionDenied {
                code: P::CODE.into(),
            })
        }
    }
}

/// Permission marker: `tenants.create`.
pub struct TenantsCreate;
impl Permission for TenantsCreate {
    const CODE: &'static str = "tenants.create";
}

/// Permission marker: `tenants.members.list`.
/// Accepts either the direct permission OR operator bypass (`tenants.manage_all`).
pub struct TenantsMembersList;
impl Permission for TenantsMembersList {
    const CODE: &'static str = "tenants.members.list";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: `tenants.roles.assign`.
/// Accepts either the direct permission OR operator bypass (`tenants.manage_all`).
pub struct TenantsRolesAssign;
impl Permission for TenantsRolesAssign {
    const CODE: &'static str = "tenants.roles.assign";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: `users.manage_all` — platform-level user administration.
pub struct UsersManageAll;
impl Permission for UsersManageAll {
    const CODE: &'static str = "users.manage_all";
}

/// Permission marker: `tenants.members.add`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct TenantsMembersAdd;
impl Permission for TenantsMembersAdd {
    const CODE: &'static str = "tenants.members.add";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: `tenants.members.remove`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct TenantsMembersRemove;
impl Permission for TenantsMembersRemove {
    const CODE: &'static str = "tenants.members.remove";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: list platform users.
/// CODE is `tenants.members.list` (the existing DB permission for tenant admins) rather
/// than a new `users.read` permission, intentionally avoiding a schema migration.
/// `accepts()` also grants access to operators via `users.manage_all`.
pub struct UsersRead;
impl Permission for UsersRead {
    const CODE: &'static str = "tenants.members.list";
    fn accepts(set: &PermissionSet) -> bool {
        set.has("tenants.members.list") || set.is_operator_over_users()
    }
}

/// Permission marker: `channels.manage`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct ChannelsManage;
impl Permission for ChannelsManage {
    const CODE: &'static str = "channels.manage";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}

/// Permission marker: `service_accounts.read`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct ServiceAccountsRead;
impl Permission for ServiceAccountsRead {
    const CODE: &'static str = "service_accounts.read";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.has("service_accounts.manage") || set.is_operator_over_tenants()
    }
}

/// Permission marker: `service_accounts.manage`.
/// Accepts either the direct permission OR `tenants.manage_all` operator bypass.
pub struct ServiceAccountsManage;
impl Permission for ServiceAccountsManage {
    const CODE: &'static str = "service_accounts.manage";
    fn accepts(set: &PermissionSet) -> bool {
        set.has(Self::CODE) || set.is_operator_over_tenants()
    }
}
