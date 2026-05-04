use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::common::auth::bearer;
use crate::common::seed::{grant_permission_to_role, grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

/// Seed an org_admin user in the given org and return (user_id, org_id, bearer_token).
async fn org_admin_user(
    pool: &sqlx::PgPool,
    username: &str,
    org_name: &str,
) -> (Uuid, Uuid, String) {
    let user = seed_user(pool, username).await;
    let org = seed_org(pool, org_name, "retail").await;
    grant_role(pool, user, org, "org_admin").await;
    let cfg = test_config();
    let auth = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, user, org);
    (user, org, auth)
}

/// Create a service account via the HTTP API and return its user_id.
async fn make_sa(cli: &reqwest::Client, base: &str, auth: &str, org: Uuid, name: &str) -> Uuid {
    let resp = cli
        .post(format!("{base}/api/v1/security/service-accounts"))
        .header("authorization", auth)
        .json(&json!({"organisation_id": org, "name": name, "description": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    Uuid::parse_str(body["user_id"].as_str().unwrap()).unwrap()
}

/// Mint an API key for the SA and return (key_id, plaintext).
async fn mint_key(
    cli: &reqwest::Client,
    base: &str,
    auth: &str,
    sa_id: Uuid,
    scopes: serde_json::Value,
) -> (Uuid, String) {
    let resp = cli
        .post(format!(
            "{base}/api/v1/security/service-accounts/{sa_id}/api-keys"
        ))
        .header("authorization", auth)
        .json(&json!({"name": "primary", "scopes": scopes}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let key_id = Uuid::parse_str(body["key"]["id"].as_str().unwrap()).unwrap();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();
    (key_id, plaintext)
}

// ── Test 1 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn post_echo_with_api_key_and_echo_invoke_permission_returns_200_and_payload_round_trips() {
    let pool = TestPool::fresh().await.pool;
    let (_admin_user, org, admin_auth) = org_admin_user(&pool, "alice", "acme").await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    // Create SA and mint a key with echo:invoke scope.
    let sa_id = make_sa(&cli, &app.base_url, &admin_auth, org, "echo-bot").await;
    let (_key_id, plaintext) = mint_key(
        &cli,
        &app.base_url,
        &admin_auth,
        sa_id,
        json!(["echo:invoke"]),
    )
    .await;

    // Grant echo:invoke to a role and assign that role to the SA user so
    // the permission loader sees it.
    grant_role(&pool, sa_id, org, "org_member").await;
    grant_permission_to_role(&pool, "org_member", "echo:invoke").await;

    // Retrieve the actual key_id from the DB so we can assert it.
    let key_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM api_keys WHERE service_account_user_id = $1 AND revoked_at IS NULL",
    )
    .bind(sa_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let payload = json!({"hello": "world"});
    let resp = cli
        .post(format!("{}/api/v1/echo", app.base_url))
        .bearer_auth(&plaintext)
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "expected 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "POST");
    assert_eq!(body["payload"], payload);
    assert_eq!(body["org_id"], org.to_string());
    assert_eq!(body["key_id"], key_id.to_string());
    assert_eq!(body["principal_user_id"], sa_id.to_string());
    assert!(
        body["received_at"].is_string(),
        "received_at must be present"
    );

    app.stop().await;
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn post_echo_with_api_key_lacking_echo_invoke_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let (_admin_user, org, admin_auth) = org_admin_user(&pool, "bob", "betacorp").await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    // Mint key with a different scope (no echo:invoke).
    let sa_id = make_sa(&cli, &app.base_url, &admin_auth, org, "noperm-bot").await;
    let (_key_id, plaintext) = mint_key(
        &cli,
        &app.base_url,
        &admin_auth,
        sa_id,
        json!(["other:scope"]),
    )
    .await;
    // SA has org_member role but org_member does NOT have echo:invoke by default.
    grant_role(&pool, sa_id, org, "org_member").await;

    let resp = cli
        .post(format!("{}/api/v1/echo", app.base_url))
        .bearer_auth(&plaintext)
        .json(&json!({"test": "no_perm"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["type"]
            .as_str()
            .unwrap_or("")
            .ends_with("permission.denied"),
        "expected permission.denied slug, got {:?}",
        body["type"]
    );

    app.stop().await;
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn post_echo_without_credentials_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .post(format!("{}/api/v1/echo", app.base_url))
        .json(&json!({"hello": "world"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}

// ── Test 4 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_echo_with_jwt_user_returns_200_with_null_key_id() {
    let pool = TestPool::fresh().await.pool;
    let (user_id, org, auth) = org_admin_user(&pool, "carol", "carolcorp").await;
    // Grant echo:invoke to org_admin so the JWT user can call the endpoint.
    grant_permission_to_role(&pool, "org_admin", "echo:invoke").await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let resp = cli
        .get(format!("{}/api/v1/echo", app.base_url))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "expected 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "GET");
    assert!(body["payload"].is_null(), "GET payload must be null");
    assert_eq!(body["org_id"], org.to_string());
    assert!(
        body["key_id"].is_null(),
        "key_id must be null for JWT callers"
    );
    assert_eq!(body["principal_user_id"], user_id.to_string());

    app.stop().await;
}

// ── Test 5 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn post_echo_with_jwt_user_returns_payload() {
    let pool = TestPool::fresh().await.pool;
    let (user_id, org, auth) = org_admin_user(&pool, "dave", "davecorp").await;
    // Grant echo:invoke to org_admin so the JWT user can call the endpoint.
    grant_permission_to_role(&pool, "org_admin", "echo:invoke").await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let payload = json!({"rust": "TDD"});
    let resp = cli
        .post(format!("{}/api/v1/echo", app.base_url))
        .header("authorization", &auth)
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "expected 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["method"], "POST");
    assert_eq!(body["payload"], payload);
    assert_eq!(body["org_id"], org.to_string());
    assert!(
        body["key_id"].is_null(),
        "key_id must be null for JWT callers"
    );
    assert_eq!(body["principal_user_id"], user_id.to_string());

    app.stop().await;
}
