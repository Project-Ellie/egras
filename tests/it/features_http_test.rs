use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::common::auth::bearer;
use crate::common::fixtures::OPERATOR_ORG_ID;
use crate::common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

/// Seed a self-service feature flag directly (not in the standard migration catalog).
async fn seed_self_service_flag(pool: &sqlx::PgPool, slug: &str) {
    sqlx::query(
        "INSERT INTO feature_definitions (slug, value_type, default_value, description, self_service) \
         VALUES ($1, 'bool', 'false'::jsonb, 'Test self-service flag', true) \
         ON CONFLICT (slug) DO NOTHING",
    )
    .bind(slug)
    .execute(pool)
    .await
    .expect("seed self-service flag");
}

/// Seed an org_admin user in the given org and return (user_id, org_id, bearer_token).
async fn org_admin(pool: &sqlx::PgPool, username: &str, org_name: &str) -> (Uuid, Uuid, String) {
    let user = seed_user(pool, username).await;
    let org = seed_org(pool, org_name, "retail").await;
    grant_role(pool, user, org, "org_admin").await;
    let cfg = test_config();
    let auth = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    (user, org, auth)
}

/// Seed an operator_admin user in OPERATOR_ORG and return (user_id, bearer_token).
async fn operator_admin(pool: &sqlx::PgPool, username: &str) -> (Uuid, String) {
    let user = seed_user(pool, username).await;
    grant_role(pool, user, OPERATOR_ORG_ID, "operator_admin").await;
    let cfg = test_config();
    let auth = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, OPERATOR_ORG_ID);
    (user, auth)
}

// ── Test 1 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_features_as_org_admin_returns_full_list() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .get(format!("{}/api/v1/features/orgs/{org}", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body.as_array().expect("response should be array");
    // The migration seeds auth.api_key_headers, so at least one item must exist.
    assert!(
        !items.is_empty(),
        "expected at least one feature definition"
    );
    // Every item must have the required fields.
    for item in items {
        assert!(item["slug"].is_string());
        assert!(item["value_type"].is_string());
        assert!(!item["source"].is_null());
    }

    app.stop().await;
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_features_other_org_as_non_operator_returns_404() {
    let pool = TestPool::fresh().await.pool;
    let (_user, _org, auth) = org_admin(&pool, "alice", "acme").await;
    let other_org = seed_org(&pool, "rival", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .get(format!("{}/api/v1/features/orgs/{other_org}", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    app.stop().await;
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn put_self_service_flag_as_org_admin_succeeds_and_reflects_in_get() {
    let pool = TestPool::fresh().await.pool;
    seed_self_service_flag(&pool, "ui.dark_mode").await;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // PUT
    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{org}/ui.dark_mode",
            app.base_url
        ))
        .header("authorization", &auth)
        .json(&json!({"value": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["slug"], "ui.dark_mode");
    assert_eq!(body["value"], true);
    assert_eq!(body["source"], "override");

    // GET — confirm override persisted.
    let resp = cli
        .get(format!("{}/api/v1/features/orgs/{org}", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list: serde_json::Value = resp.json().await.unwrap();
    let flag = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["slug"] == "ui.dark_mode")
        .expect("ui.dark_mode not in list");
    assert_eq!(flag["value"], true);
    assert_eq!(flag["source"], "override");

    app.stop().await;
}

// ── Test 4 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn put_non_self_service_flag_as_org_admin_returns_403_feature_not_self_service() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // auth.api_key_headers is NOT self_service (seeded in migration 0012).
    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{org}/auth.api_key_headers",
            app.base_url
        ))
        .header("authorization", &auth)
        .json(&json!({"value": ["x-api-key"]}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"]
            .as_str()
            .unwrap_or("")
            .split('/')
            .next_back()
            .unwrap_or(""),
        "feature.not_self_service"
    );

    app.stop().await;
}

// ── Test 5 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn put_any_flag_as_operator_succeeds() {
    let pool = TestPool::fresh().await.pool;
    let (_op_user, op_auth) = operator_admin(&pool, "op_alice").await;
    let tenant_org = seed_org(&pool, "tenant_co", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // Operator sets a non-self_service flag on another org.
    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{tenant_org}/auth.api_key_headers",
            app.base_url
        ))
        .header("authorization", &op_auth)
        .json(&json!({"value": ["x-api-key"]}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["slug"], "auth.api_key_headers");
    assert_eq!(body["source"], "override");

    app.stop().await;
}

// ── Test 6 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn put_with_type_mismatch_returns_400_feature_invalid_value() {
    let pool = TestPool::fresh().await.pool;
    seed_self_service_flag(&pool, "ui.dark_mode2").await;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // ui.dark_mode2 is `bool`; sending a string is invalid.
    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{org}/ui.dark_mode2",
            app.base_url
        ))
        .header("authorization", &auth)
        .json(&json!({"value": "not_a_bool"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"]
            .as_str()
            .unwrap_or("")
            .split('/')
            .next_back()
            .unwrap_or(""),
        "feature.invalid_value"
    );

    app.stop().await;
}

// ── Test 7 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn put_unknown_slug_returns_404_feature_unknown() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{org}/no.such.flag",
            app.base_url
        ))
        .header("authorization", &auth)
        .json(&json!({"value": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"]
            .as_str()
            .unwrap_or("")
            .split('/')
            .next_back()
            .unwrap_or(""),
        "feature.unknown"
    );

    app.stop().await;
}

// ── Test 8 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_override_returns_204_and_get_shows_default_source() {
    let pool = TestPool::fresh().await.pool;
    seed_self_service_flag(&pool, "ui.dark_mode3").await;
    let (_user, org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    // First set an override.
    let resp = cli
        .put(format!(
            "{}/api/v1/features/orgs/{org}/ui.dark_mode3",
            app.base_url
        ))
        .header("authorization", &auth)
        .json(&json!({"value": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // DELETE the override.
    let resp = cli
        .delete(format!(
            "{}/api/v1/features/orgs/{org}/ui.dark_mode3",
            app.base_url
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // GET — source must now be "default".
    let resp = cli
        .get(format!("{}/api/v1/features/orgs/{org}", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list: serde_json::Value = resp.json().await.unwrap();
    let flag = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["slug"] == "ui.dark_mode3")
        .expect("ui.dark_mode3 not in list");
    assert_eq!(flag["source"], "default");
    assert_eq!(flag["value"], false);

    app.stop().await;
}

// ── Bonus: operator catalog endpoint ─────────────────────────────────────────

#[tokio::test]
async fn get_definitions_as_operator_returns_catalog() {
    let pool = TestPool::fresh().await.pool;
    let (_op_user, op_auth) = operator_admin(&pool, "op_bob").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .get(format!("{}/api/v1/features", app.base_url))
        .header("authorization", &op_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let items = body.as_array().expect("response should be array");
    assert!(!items.is_empty());
    for item in items {
        assert!(item["slug"].is_string());
        assert!(item["value_type"].is_string());
        assert!(item["self_service"].is_boolean());
    }

    app.stop().await;
}

#[tokio::test]
async fn get_definitions_as_non_operator_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let (_user, _org, auth) = org_admin(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .get(format!("{}/api/v1/features", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    app.stop().await;
}
