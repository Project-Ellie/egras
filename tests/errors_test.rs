use axum::body::to_bytes;
use axum::response::IntoResponse;
use egras::errors::{AppError, ErrorSlug};
use serde_json::Value;

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

#[tokio::test]
async fn permission_denied_serialises_as_rfc7807() {
    let err = AppError::PermissionDenied { code: "tenants.members.add".into() };
    let resp = err.into_response();
    assert_eq!(resp.status().as_u16(), 403);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.starts_with("application/problem+json"));

    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["type"], "https://egras.dev/errors/permission.denied");
    assert_eq!(v["title"], "Permission denied");
    assert_eq!(v["status"], 403);
    assert_eq!(v["detail"], "missing permission: tenants.members.add");
}

#[tokio::test]
async fn validation_error_includes_errors_map() {
    let mut errors = std::collections::HashMap::new();
    errors.insert("email".to_string(), vec!["format".to_string()]);
    let err = AppError::Validation { errors };
    let resp = err.into_response();
    assert_eq!(resp.status().as_u16(), 400);
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["type"], "https://egras.dev/errors/validation.invalid_request");
    assert_eq!(v["errors"]["email"][0], "format");
}
