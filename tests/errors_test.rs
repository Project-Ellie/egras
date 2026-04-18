use egras::errors::{AppError, ErrorSlug};

#[test]
fn slug_of_permission_denied_is_stable() {
    let err = AppError::PermissionDenied { code: "tenants.members.add".into() };
    assert_eq!(err.slug(), ErrorSlug::PermissionDenied);
    assert_eq!(err.http_status().as_u16(), 403);
}

#[test]
fn slug_of_validation_is_400() {
    let err = AppError::Validation { errors: Default::default() };
    assert_eq!(err.http_status().as_u16(), 400);
}

#[test]
fn slug_of_unauthenticated_is_401() {
    let err = AppError::Unauthenticated { reason: "missing_header".into() };
    assert_eq!(err.http_status().as_u16(), 401);
}

#[test]
fn slug_of_not_found_is_404() {
    let err = AppError::NotFound { resource: "organisation".into() };
    assert_eq!(err.http_status().as_u16(), 404);
}

#[test]
fn slug_of_conflict_is_409() {
    let err = AppError::Conflict { reason: "last_owner".into() };
    assert_eq!(err.http_status().as_u16(), 409);
}

#[test]
fn internal_error_is_500() {
    let err = AppError::Internal(anyhow::anyhow!("boom"));
    assert_eq!(err.http_status().as_u16(), 500);
}
