#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use common::auth::bearer;
use common::seed::{grant_role, seed_org, seed_user_with_password};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn logout_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "logout_user", "pass1234").await;
    let org = seed_org(&pool, "logout-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn logout_unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn change_password_happy_path_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "cp_user", "oldpass1").await;
    let org = seed_org(&pool, "cp-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/change-password", app.base_url))
        .header("authorization", &token)
        .json(&json!({
            "current_password": "oldpass1",
            "new_password": "newpass99"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn change_password_wrong_current_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "cp_user2", "correct").await;
    let org = seed_org(&pool, "cp-org2", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/change-password", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "current_password": "wrong",
            "new_password": "newpass99"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn switch_org_happy_path_returns_200_with_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "switch_user", "pass1234").await;
    let org1 = seed_org(&pool, "switch-org1", "retail").await;
    let org2 = seed_org(&pool, "switch-org2", "media").await;
    grant_role(&pool, user, org1, "org_member").await;
    grant_role(&pool, user, org2, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/switch-org", app.base_url))
        .header("authorization", token)
        .json(&json!({ "org_id": org2 }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    app.stop().await;
}

#[tokio::test]
async fn switch_org_not_member_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "switch_user2", "pass1234").await;
    let org1 = seed_org(&pool, "switch2-org1", "retail").await;
    let other_org = seed_org(&pool, "switch2-other", "media").await;
    grant_role(&pool, user, org1, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/switch-org", app.base_url))
        .header("authorization", token)
        .json(&json!({ "org_id": other_org }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn logout_revokes_token_subsequent_request_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "revoke_http_user", "pass1234").await;
    let org = seed_org(&pool, "revoke-http-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;
    let client = reqwest::Client::new();

    // Logout succeeds.
    let resp = client
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Same token is now rejected.
    let resp = client
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}
