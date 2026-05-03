use egras::config::AppConfig;
use egras::security::service::bootstrap_seed_admin::{bootstrap_seed_admin, SeedAdminInput};
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn seed_admin_then_login_succeeds() {
    let pool = TestPool::fresh().await.pool;

    // Seed the admin user directly via the service.
    bootstrap_seed_admin(
        &pool,
        SeedAdminInput {
            email: "admin@example.com".into(),
            username: "admin".into(),
            password: "hunter2hunter2".into(),
            role_code: "operator_admin".into(),
            operator_org_name: "operator".into(),
        },
    )
    .await
    .expect("seed admin");

    // Spin up the HTTP server.
    let cfg = AppConfig::default_for_tests();
    let app = TestApp::spawn(pool, cfg).await;

    // Login with the seeded credentials.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({
            "username_or_email": "admin@example.com",
            "password": "hunter2hunter2"
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
