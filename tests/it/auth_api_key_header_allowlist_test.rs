/// Tests for the per-org `auth.api_key_headers` allowlist.
///
/// Strategy: insert the override into `organisation_features` *before*
/// `TestApp::spawn` so the middleware's evaluator sees it on first cache miss
/// (no invalidation needed across process boundaries).
use egras::config::AppConfig;
use egras::security::persistence::{
    ApiKeyRepository, ApiKeyRepositoryPg, NewApiKeyRow, NewServiceAccount,
    ServiceAccountRepository, ServiceAccountRepositoryPg,
};
use egras::security::service::api_key_secret;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::common::seed::{grant_permission_to_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

/// Seed a service account in `org`, assign it `role_code`, create an API key,
/// and return `(sa_user_id, plaintext_key)`.
async fn seed_sa_with_role(pool: &sqlx::PgPool, org: Uuid, role_code: &str) -> (Uuid, String) {
    let creator = seed_user(pool, &format!("creator-{}", Uuid::now_v7().simple())).await;
    let sa = ServiceAccountRepositoryPg::new(pool.clone())
        .create(NewServiceAccount {
            organisation_id: org,
            name: format!("bot-{}", Uuid::now_v7().simple()),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();

    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
        .bind(role_code)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(sa.user_id)
    .bind(org)
    .bind(role_id)
    .execute(pool)
    .await
    .unwrap();

    let g = api_key_secret::generate().unwrap();
    ApiKeyRepositoryPg::new(pool.clone())
        .create(NewApiKeyRow {
            id: Uuid::now_v7(),
            service_account_user_id: sa.user_id,
            prefix: g.prefix.clone(),
            secret_hash: api_key_secret::hash_secret(&g.secret).unwrap(),
            name: "primary".into(),
            scopes: Some(vec!["echo:invoke".into()]),
            created_by: creator,
        })
        .await
        .unwrap();

    (sa.user_id, g.plaintext)
}

/// Write an org-feature override directly into `organisation_features`.
/// Bypasses the service layer (and the evaluator cache on the *test* side),
/// which is fine — the middleware's evaluator will hit the DB on first read.
async fn set_allowlist_override(pool: &sqlx::PgPool, org: Uuid, value: serde_json::Value) {
    let actor = seed_user(pool, &format!("actor-{}", Uuid::now_v7().simple())).await;
    sqlx::query(
        "INSERT INTO organisation_features (organisation_id, slug, value, updated_by) \
         VALUES ($1, 'auth.api_key_headers', $2, $3) \
         ON CONFLICT (organisation_id, slug) DO UPDATE \
           SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = NOW()",
    )
    .bind(org)
    .bind(sqlx::types::Json(&value))
    .bind(actor)
    .execute(pool)
    .await
    .unwrap();
}

// ── Test 1 ────────────────────────────────────────────────────────────────────

/// Default allowlist accepts both `X-API-Key` and `Authorization: Bearer`.
#[tokio::test]
async fn default_allowlist_accepts_both_headers() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme-default", "retail").await;
    let (sa_id, plaintext) = seed_sa_with_role(&pool, org, "org_member").await;
    grant_permission_to_role(&pool, "org_member", "echo:invoke").await;

    // No override — default ["x-api-key","authorization-bearer"] applies.
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();
    let echo_url = format!("{}/api/v1/echo", app.base_url);

    // Authorization: Bearer <key>
    let resp = cli
        .get(&echo_url)
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Authorization: Bearer should be accepted by default; sa={sa_id}"
    );

    // X-API-Key: <key>
    let resp = cli
        .get(&echo_url)
        .header("x-api-key", &plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "X-API-Key should be accepted by default"
    );

    app.stop().await;
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

/// Allowlist overridden to `["x-api-key"]`: Bearer is rejected, X-API-Key succeeds.
#[tokio::test]
async fn override_to_x_api_key_only_rejects_authorization_bearer() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme-xapikey", "retail").await;
    let (_sa_id, plaintext) = seed_sa_with_role(&pool, org, "org_member").await;
    grant_permission_to_role(&pool, "org_member", "echo:invoke").await;

    // Set override BEFORE spawning the app so the evaluator sees it cold.
    set_allowlist_override(&pool, org, json!(["x-api-key"])).await;

    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();
    let echo_url = format!("{}/api/v1/echo", app.base_url);

    // X-API-Key should succeed.
    let resp = cli
        .get(&echo_url)
        .header("x-api-key", &plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "X-API-Key must still be accepted when allowlist = [\"x-api-key\"]"
    );

    // Authorization: Bearer must be rejected.
    let resp = cli
        .get(&echo_url)
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Authorization: Bearer must be rejected when not in allowlist"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"], "https://egras.dev/errors/auth.unauthenticated",
        "error type must be auth.unauthenticated"
    );

    app.stop().await;
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

/// Allowlist overridden to `["authorization-bearer"]`: X-API-Key is rejected, Bearer succeeds.
#[tokio::test]
async fn override_to_authorization_bearer_only_rejects_x_api_key() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme-bearer", "retail").await;
    let (_sa_id, plaintext) = seed_sa_with_role(&pool, org, "org_member").await;
    grant_permission_to_role(&pool, "org_member", "echo:invoke").await;

    set_allowlist_override(&pool, org, json!(["authorization-bearer"])).await;

    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();
    let echo_url = format!("{}/api/v1/echo", app.base_url);

    // Authorization: Bearer should succeed.
    let resp = cli
        .get(&echo_url)
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Authorization: Bearer must be accepted when allowlist = [\"authorization-bearer\"]"
    );

    // X-API-Key must be rejected.
    let resp = cli
        .get(&echo_url)
        .header("x-api-key", &plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "X-API-Key must be rejected when not in allowlist"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"], "https://egras.dev/errors/auth.unauthenticated",
        "error type must be auth.unauthenticated"
    );

    app.stop().await;
}

// ── Test 4 ────────────────────────────────────────────────────────────────────

/// X-API-Key header with an invalid/unknown token returns 401.
#[tokio::test]
async fn x_api_key_header_with_invalid_token_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    // Well-formed key format but unknown prefix — verifier will return None.
    let bogus = "egras_live_aaaaaaaa_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

    let resp = cli
        .get(format!("{}/api/v1/echo", app.base_url))
        .header("x-api-key", bogus)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"], "https://egras.dev/errors/auth.unauthenticated",
        "expected auth.unauthenticated error"
    );

    app.stop().await;
}

// ── Test 5 ────────────────────────────────────────────────────────────────────

/// A JWT presented in `X-API-Key` is rejected because X-API-Key cannot carry a JWT.
#[tokio::test]
async fn x_api_key_header_cannot_carry_jwt() {
    let pool = TestPool::fresh().await.pool;
    let cfg = test_config();

    // Mint a JWT directly (no user needed — the middleware will reject before
    // the JWT is even decoded on the X-API-Key path).
    let jwt = egras::testing::mint_jwt(
        &cfg.jwt_secret,
        &cfg.jwt_issuer,
        Uuid::now_v7(),
        Uuid::now_v7(),
        3600,
    );

    let app = TestApp::spawn(pool.clone(), cfg).await;
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/echo", app.base_url))
        .header("x-api-key", &jwt)
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "JWT in X-API-Key must be rejected"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"], "https://egras.dev/errors/auth.unauthenticated",
        "error type must be auth.unauthenticated"
    );

    app.stop().await;
}
