#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401_problem_json() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .json(&json!({ "name": "acme", "business": "retail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/auth.unauthenticated"
    );
    app.stop().await;
}

#[tokio::test]
async fn missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "acme", "business": "retail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/permission.denied");
    app.stop().await;
}

#[tokio::test]
async fn happy_path_creates_org_and_returns_201() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool.clone(), cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "beta", "business": "media" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "beta");
    assert_eq!(body["business"], "media");
    assert_eq!(body["role_codes"], json!(["org_owner"]));

    app.stop().await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'organisation.created'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn malformed_body_with_missing_permission_still_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await; // lacks tenants.create

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .header("content-type", "application/json")
        .body("this is not valid json {")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/permission.denied");
    app.stop().await;
}

#[tokio::test]
async fn duplicate_name_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let seed = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, seed, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, seed);
    let app = TestApp::spawn(pool, cfg).await;

    let _ = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", &token)
        .json(&json!({ "name": "clash", "business": "retail" }))
        .send()
        .await
        .unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/tenants/organisations", app.base_url))
        .header("authorization", token)
        .json(&json!({ "name": "clash", "business": "media" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/resource.conflict");
    app.stop().await;
}
