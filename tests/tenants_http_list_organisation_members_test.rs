#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{}/members",
            app.base_url, org
        ))
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
async fn happy_path_returns_members() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    grant_role(&pool, bob, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{}/members",
            app.base_url, org
        ))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(body["next_cursor"].is_null());
    app.stop().await;
}

#[tokio::test]
async fn non_member_non_operator_gets_404() {
    // Alternative per plan: assert non-member + non-operator returns 404 (resource.not_found).
    // mallory is granted org_member in a separate org so they have tenants.members.list,
    // but they are NOT a member of the target org. The service layer enforces 404.
    let pool = TestPool::fresh().await.pool;
    let mallory = seed_user(&pool, "mallory").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let mallory_other_org = seed_org(&pool, "mallory-corp", "media").await;
    grant_role(&pool, alice, org, "org_owner").await;
    grant_role(&pool, mallory, mallory_other_org, "org_member").await;

    let cfg = test_config();
    // mallory's home org is mallory_other_org; they have tenants.members.list
    // but are NOT a member of the target 'org'.
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, mallory, mallory_other_org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{}/members",
            app.base_url, org
        ))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/resource.not_found");
    app.stop().await;
}

#[tokio::test]
async fn invalid_cursor_returns_400() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{}/members?after=not-base64",
            app.base_url, org
        ))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/validation.invalid_request"
    );
    let errors = &body["errors"];
    assert_eq!(errors["after"][0], "invalid_cursor");
    app.stop().await;
}
