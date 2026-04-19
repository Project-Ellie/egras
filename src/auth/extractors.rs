use std::marker::PhantomData;

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::auth::jwt::Claims;
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

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
