use std::collections::HashSet;

use async_trait::async_trait;
use axum::{extract::FromRequestParts, http::request::Parts};
use uuid::Uuid;

use crate::auth::jwt::Claims;
use crate::errors::AppError;

#[derive(Debug, Clone, Default)]
pub struct PermissionSet {
    codes: HashSet<String>,
}

impl PermissionSet {
    pub fn from_codes<I: IntoIterator<Item = String>>(codes: I) -> Self {
        Self {
            codes: codes.into_iter().collect(),
        }
    }

    pub fn has(&self, code: &str) -> bool {
        self.codes.contains(code)
    }

    pub fn has_any(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.codes.contains(*c))
    }

    pub fn is_operator_over_tenants(&self) -> bool {
        self.has("tenants.manage_all")
    }

    pub fn is_operator_over_users(&self) -> bool {
        self.has("users.manage_all")
    }

    pub fn is_audit_read_all(&self) -> bool {
        self.has("audit.read_all")
    }

    /// Iterate codes (deterministic order for logging/testing).
    pub fn iter_sorted(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.codes.iter().map(String::as_str).collect();
        v.sort();
        v
    }

    /// Restrict to the intersection of `self` and `allowed`. Used by the
    /// API-key auth path to apply per-key `scopes` to the SA's loaded
    /// permissions: `effective = sa_perms ∩ key.scopes`.
    pub fn intersect(&self, allowed: &[String]) -> Self {
        let allow: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        Self {
            codes: self
                .codes
                .iter()
                .filter(|c| allow.contains(c.as_str()))
                .cloned()
                .collect(),
        }
    }
}

/// Axum extractor that enforces the caller holds `code`.
pub struct RequirePermission(pub &'static str);

#[async_trait]
impl<S> FromRequestParts<S> for RequirePermission
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // This is a marker extractor; real usage is `require(parts, "code")` in handlers,
        // but we preserve the struct form for ergonomics. See `require_permission`.
        let _ = parts;
        // Caller should never instantiate without specifying the code; this branch
        // intentionally rejects the marker usage to catch misuse.
        Err(AppError::Internal(anyhow::anyhow!(
            "RequirePermission must not be used as a bare extractor; call `require_permission`."
        )))
    }
}

/// Call from a handler: `require_permission(&parts, "tenants.members.add")?;`
pub fn require_permission(parts: &Parts, code: &'static str) -> Result<(), AppError> {
    let set = parts
        .extensions
        .get::<PermissionSet>()
        .ok_or_else(|| AppError::Unauthenticated {
            reason: "no_permission_set".into(),
        })?;
    if set.has(code) {
        Ok(())
    } else {
        Err(AppError::PermissionDenied {
            code: code.to_string(),
        })
    }
}

/// Extract the caller's JWT `org_id` and enforce the cross-org rule from spec §3.5.
///
/// If the caller has `*.manage_all` or `audit.read_all`, they may operate on any org;
/// otherwise mismatched `organisation_id` → 404 (via `AppError::NotFound`).
pub fn authorise_org(parts: &Parts, organisation_id: Uuid) -> Result<(), AppError> {
    let claims = parts
        .extensions
        .get::<Claims>()
        .ok_or_else(|| AppError::Unauthenticated {
            reason: "no_claims".into(),
        })?;
    let set = parts
        .extensions
        .get::<PermissionSet>()
        .ok_or_else(|| AppError::Unauthenticated {
            reason: "no_permission_set".into(),
        })?;
    if set.is_operator_over_tenants() || set.is_operator_over_users() || set.is_audit_read_all() {
        return Ok(());
    }
    if claims.org == organisation_id {
        Ok(())
    } else {
        Err(AppError::NotFound {
            resource: "organisation".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_keeps_only_allowed() {
        let p = PermissionSet::from_codes(["a".to_string(), "b".to_string(), "c".to_string()]);
        let r = p.intersect(&["a".to_string(), "c".to_string(), "d".to_string()]);
        let codes = r.iter_sorted();
        assert_eq!(codes, vec!["a", "c"]);
    }

    #[test]
    fn intersect_with_empty_yields_empty() {
        let p = PermissionSet::from_codes(["a".to_string()]);
        let r = p.intersect(&[]);
        assert!(r.iter_sorted().is_empty());
    }
}
