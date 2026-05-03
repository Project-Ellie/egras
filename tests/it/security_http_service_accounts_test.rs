use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::common::auth::bearer;
use crate::common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

async fn human_admin(pool: &sqlx::PgPool) -> (Uuid, Uuid, String) {
    let user = seed_user(pool, "alice").await;
    let org = seed_org(pool, "acme", "retail").await;
    grant_role(pool, user, org, "org_admin").await;
    let cfg = test_config();
    let auth = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    (user, org, auth)
}

#[tokio::test]
async fn create_then_get_then_delete_round_trip() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // POST
    let resp = cli
        .post(format!("{}/api/v1/security/service-accounts", app.base_url))
        .header("authorization", &auth)
        .json(&json!({
            "organisation_id": org,
            "name": "billing-bot",
            "description": "test",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: serde_json::Value = resp.json().await.unwrap();
    let sa_id = Uuid::parse_str(created["user_id"].as_str().unwrap()).unwrap();

    // GET
    let resp = cli
        .get(format!(
            "{}/api/v1/security/service-accounts/{}",
            app.base_url, sa_id
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "billing-bot");

    // DELETE
    let resp = cli
        .delete(format!(
            "{}/api/v1/security/service-accounts/{}",
            app.base_url, sa_id
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // GET again → 404
    let resp = cli
        .get(format!(
            "{}/api/v1/security/service-accounts/{}",
            app.base_url, sa_id
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    app.stop().await;
}

#[tokio::test]
async fn unauth_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "x", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/service-accounts", app.base_url))
        .json(&json!({"organisation_id": org, "name": "bot", "description": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "member-only").await;
    let org = seed_org(&pool, "x", "retail").await;
    grant_role(&pool, user, org, "org_member").await;
    let cfg = test_config();
    let auth = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/security/service-accounts", app.base_url))
        .header("authorization", auth)
        .json(&json!({"organisation_id": org, "name": "bot", "description": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}
