use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::seed::{grant_role, seed_org, seed_user_with_password};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn login_happy_path_returns_200_with_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "alice_http", "hunter2").await;
    let org = seed_org(&pool, "alice-http-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({
            "username_or_email": "alice_http",
            "password": "hunter2"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert!(!body["token"].as_str().unwrap().is_empty());

    app.stop().await;
}

#[tokio::test]
async fn login_wrong_password_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "bob_http", "correct").await;
    let org = seed_org(&pool, "bob-http-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({
            "username_or_email": "bob_http",
            "password": "wrong"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}

#[tokio::test]
async fn login_unknown_user_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({
            "username_or_email": "nobody",
            "password": "x"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}
