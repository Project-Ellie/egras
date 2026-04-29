#[path = "common/mod.rs"]
mod common;

use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn password_reset_request_unknown_email_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/security/password-reset-request",
            app.base_url
        ))
        .json(&json!({ "email": "nobody@example.com" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn password_reset_confirm_invalid_token_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/security/password-reset-confirm",
            app.base_url
        ))
        .json(&json!({
            "token": hex::encode([0u8; 32]),
            "new_password": "newpass123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}
