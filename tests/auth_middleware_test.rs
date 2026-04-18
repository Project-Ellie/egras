use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use egras::auth::jwt::{encode_access_token, Claims};
use egras::auth::middleware::AuthLayer;
use egras::auth::permissions::PermissionSet;
use tower::ServiceExt;
use uuid::Uuid;

async fn echo_handler(
    axum::Extension(claims): axum::Extension<Claims>,
    axum::Extension(perms): axum::Extension<PermissionSet>,
) -> String {
    format!("{} {:?}", claims.sub, perms.iter_sorted())
}

fn router_with_static_permissions() -> Router {
    // For unit tests of the middleware we provide a "static" permission loader
    // that returns a fixed set regardless of user/org. The real loader is
    // exercised by the integration test on a real DB (Task 23+ in later plans).
    let secret = "a".repeat(64);
    let loader = egras::auth::middleware::PermissionLoader::static_codes(vec![
        "tenants.read".into(),
        "tenants.members.list".into(),
    ]);
    Router::new()
        .route("/echo", get(echo_handler))
        .layer(AuthLayer::new(secret.clone(), "egras".into(), loader))
}

#[tokio::test]
async fn rejects_missing_authorization_header() {
    let app = router_with_static_permissions();
    let resp = app
        .oneshot(Request::builder().uri("/echo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_bad_token() {
    let app = router_with_static_permissions();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/echo")
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn accepts_valid_token_and_injects_extensions() {
    let app = router_with_static_permissions();
    let secret = "a".repeat(64);
    let token =
        encode_access_token(&secret, "egras", Uuid::now_v7(), Uuid::now_v7(), 3600).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/echo")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let s = String::from_utf8(body.to_vec()).unwrap();
    assert!(s.contains("tenants.read"));
    assert!(s.contains("tenants.members.list"));
}
