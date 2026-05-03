use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::auth::bearer;
use crate::common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn register_unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "reg-unauth-org", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .json(&json!({
            "username": "newuser",
            "email": "newuser@example.com",
            "password": "password123",
            "org_id": org,
            "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn register_missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "reg_no_perm").await;
    let org = seed_org(&pool, "reg-no-perm-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await; // lacks tenants.members.add

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "username": "newuser",
            "email": "newuser@example.com",
            "password": "password123",
            "org_id": org,
            "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn register_happy_path_returns_201() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "reg_admin").await;
    let org = seed_org(&pool, "reg-admin-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/register", app.base_url))
        .header("authorization", token)
        .json(&json!({
            "username": "brandnew",
            "email": "brandnew@example.com",
            "password": "password123",
            "org_id": org,
            "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["user_id"].is_string());
    app.stop().await;
}
