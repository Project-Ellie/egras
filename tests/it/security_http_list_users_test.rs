use crate::common::auth::bearer;
use crate::common::fixtures::OPERATOR_ORG_ID;
use crate::common::seed::{grant_role, seed_org, seed_user};
use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    app.stop().await;
}

#[tokio::test]
async fn insufficient_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let cfg = test_config();
    // Alice has no org role — no permissions loaded. Use a random org UUID.
    let token = bearer(
        &cfg.jwt_secret,
        &cfg.jwt_issuer,
        alice,
        uuid::Uuid::now_v7(),
    );
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    app.stop().await;
}

#[tokio::test]
async fn operator_list_returns_full_memberships() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    // Grant alice operator_admin so she gets users.manage_all.
    grant_role(&pool, alice, OPERATOR_ORG_ID, "operator_admin").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    let alice_item = items.iter().find(|i| i["username"] == "alice").unwrap();
    let memberships = alice_item["memberships"].as_array().unwrap();
    assert!(!memberships.is_empty());
    app.stop().await;
}

#[tokio::test]
async fn tenant_admin_list_scoped_to_own_org() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org1 = seed_org(&pool, "acme", "retail").await;
    let org2 = seed_org(&pool, "globex", "media").await;
    grant_role(&pool, alice, org1, "org_owner").await;
    grant_role(&pool, bob, org2, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
    for item in items {
        for m in item["memberships"].as_array().unwrap() {
            assert_eq!(m["org_id"], org1.to_string());
        }
    }
    app.stop().await;
}

#[tokio::test]
async fn pagination_next_cursor_present_when_more_results() {
    let pool = TestPool::fresh().await.pool;
    let admin = seed_user(&pool, "admin").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, admin, org, "org_owner").await;
    for i in 0..2 {
        let u = seed_user(&pool, &format!("member{i}")).await;
        grant_role(&pool, u, org, "org_member").await;
    }

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?limit=2", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert!(!body["next_cursor"].is_null());
    app.stop().await;
}

#[tokio::test]
async fn filter_by_org_id_works() {
    let pool = TestPool::fresh().await.pool;
    let admin = seed_user(&pool, "admin").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, admin, OPERATOR_ORG_ID, "operator_admin").await;
    grant_role(&pool, alice, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?org_id={}", app.base_url, org))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
    app.stop().await;
}

#[tokio::test]
async fn search_q_filters_results() {
    let pool = TestPool::fresh().await.pool;
    let admin = seed_user(&pool, "admin").await;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;
    grant_role(&pool, admin, OPERATOR_ORG_ID, "operator_admin").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, admin, OPERATOR_ORG_ID);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?q=alice", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["username"], "alice");
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
        .get(format!("{}/api/v1/users?after=not-valid!!", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    let errors = &body["errors"];
    assert_eq!(errors["after"][0], "invalid_cursor");
    app.stop().await;
}

#[tokio::test]
async fn invalid_limit_returns_400() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?limit=0", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    let errors = &body["errors"];
    assert_eq!(errors["limit"][0], "invalid_limit");
    app.stop().await;
}
