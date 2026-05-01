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

async fn seed_channel(
    app: &TestApp,
    token: &str,
    org_id: uuid::Uuid,
    name: &str,
) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org_id}/channels",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({
            "name": name,
            "channel_type": "rest",
            "is_active": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "seed_channel failed");
    resp.json().await.unwrap()
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "anon-org", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/channels",
            app.base_url
        ))
        .json(&json!({ "name": "c1", "channel_type": "rest" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org2", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/channels",
            app.base_url
        ))
        .header("authorization", &token)
        .json(&json!({ "name": "c1", "channel_type": "rest" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn create_returns_201_with_api_key() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "bob-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/channels",
            app.base_url
        ))
        .header("authorization", &token)
        .json(&json!({ "name": "my-channel", "channel_type": "vast" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["id"].is_string());
    assert_eq!(body["name"], "my-channel");
    let api_key = body["api_key"].as_str().unwrap();
    assert_eq!(api_key.len(), 64, "api_key should be 64 chars");

    app.stop().await;
}

#[tokio::test]
async fn duplicate_name_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "carol").await;
    let org = seed_org(&pool, "carol-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    seed_channel(&app, &token, org, "dup-channel").await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/channels",
            app.base_url
        ))
        .header("authorization", &token)
        .json(&json!({ "name": "dup-channel", "channel_type": "rest" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    app.stop().await;
}

#[tokio::test]
async fn get_channel_from_different_org_returns_404() {
    let pool = TestPool::fresh().await.pool;

    // org1 owner creates a channel
    let user1 = seed_user(&pool, "dave").await;
    let org1 = seed_org(&pool, "dave-org", "retail").await;
    grant_role(&pool, user1, org1, "org_owner").await;

    // org2 owner tries to access it
    let user2 = seed_user(&pool, "eve").await;
    let org2 = seed_org(&pool, "eve-org", "retail").await;
    grant_role(&pool, user2, org2, "org_owner").await;

    let cfg = test_config();
    let token1 = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user1, org1);
    let token2 = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user2, org2);
    let app = TestApp::spawn(pool, cfg).await;

    let ch = seed_channel(&app, &token1, org1, "org1-channel").await;
    let channel_id = ch["id"].as_str().unwrap();

    // user2 tries to GET the channel scoped under org1
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{org1}/channels/{channel_id}",
            app.base_url
        ))
        .header("authorization", token2)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    app.stop().await;
}

#[tokio::test]
async fn full_lifecycle() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "frank").await;
    let org = seed_org(&pool, "frank-org", "retail").await;
    grant_role(&pool, user, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    let app = TestApp::spawn(pool, cfg).await;

    // CREATE
    let ch = seed_channel(&app, &token, org, "lifecycle-ch").await;
    let channel_id = ch["id"].as_str().unwrap();
    assert_eq!(ch["name"], "lifecycle-ch");

    // GET
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{org}/channels/{channel_id}",
            app.base_url
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], ch["id"]);

    // UPDATE
    let resp = reqwest::Client::new()
        .put(format!(
            "{}/api/v1/tenants/organisations/{org}/channels/{channel_id}",
            app.base_url
        ))
        .header("authorization", &token)
        .json(&json!({
            "name": "lifecycle-ch-updated",
            "channel_type": "sensor",
            "is_active": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(updated["name"], "lifecycle-ch-updated");
    assert_eq!(updated["is_active"], false);

    // LIST
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{org}/channels",
            app.base_url
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list: serde_json::Value = resp.json().await.unwrap();
    assert!(!list["items"].as_array().unwrap().is_empty());

    // DELETE
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/api/v1/tenants/organisations/{org}/channels/{channel_id}",
            app.base_url
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // GET after delete → 404
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/organisations/{org}/channels/{channel_id}",
            app.base_url
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    app.stop().await;
}
