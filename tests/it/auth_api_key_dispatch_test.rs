use egras::config::AppConfig;
use egras::security::persistence::{
    ApiKeyRepository, ApiKeyRepositoryPg, NewApiKeyRow, NewServiceAccount,
    ServiceAccountRepository, ServiceAccountRepositoryPg,
};
use egras::security::service::api_key_secret;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;
use uuid::Uuid;

use crate::common::seed::{seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

async fn seed_sa_with_role(pool: &sqlx::PgPool, org: Uuid, role_code: &str) -> (Uuid, String) {
    let creator = seed_user(pool, "human-creator").await;
    let sa = ServiceAccountRepositoryPg::new(pool.clone())
        .create(NewServiceAccount {
            organisation_id: org,
            name: format!("bot-{}", Uuid::now_v7().simple()),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();

    // Grant the SA a role directly (bypassing assign_role's membership check).
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
    let row = NewApiKeyRow {
        id: Uuid::now_v7(),
        service_account_user_id: sa.user_id,
        prefix: g.prefix.clone(),
        secret_hash: api_key_secret::hash_secret(&g.secret).unwrap(),
        name: "primary".into(),
        scopes: None,
        created_by: creator,
    };
    ApiKeyRepositoryPg::new(pool.clone())
        .create(row)
        .await
        .unwrap();
    (sa.user_id, g.plaintext)
}

#[tokio::test]
async fn valid_api_key_authenticates_and_resolves_permissions() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme", "retail").await;
    let (_sa, key) = seed_sa_with_role(&pool, org, "org_admin").await;

    let app = TestApp::spawn(pool.clone(), test_config()).await;

    // GET /api/v1/users requires `tenants.members.list` (granted to org_admin).
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?org_id={}", app.base_url, org))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    app.stop().await;
}

#[tokio::test]
async fn unknown_prefix_returns_401_invalid_api_key() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool.clone(), test_config()).await;

    // Well-formed key whose prefix is not in the DB.
    let bogus = "egras_live_aaaaaaaa_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .bearer_auth(bogus)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/auth.unauthenticated"
    );

    app.stop().await;
}

#[tokio::test]
async fn revoked_key_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme", "retail").await;
    let (sa, key) = seed_sa_with_role(&pool, org, "org_admin").await;

    let app = TestApp::spawn(pool.clone(), test_config()).await;

    // Revoke the SA's only key. We need the key_id; look it up.
    let key_id: Uuid =
        sqlx::query_scalar("SELECT id FROM api_keys WHERE service_account_user_id = $1 LIMIT 1")
            .bind(sa)
            .fetch_one(&pool)
            .await
            .unwrap();
    ApiKeyRepositoryPg::new(pool.clone())
        .revoke(sa, key_id)
        .await
        .unwrap();

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users", app.base_url))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}

#[tokio::test]
async fn restricted_scope_intersects_permissions() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme", "retail").await;

    // Create SA with org_admin (has tenants.members.list), then issue a key
    // restricted to `service_accounts.read` only. Key cannot use list-users.
    let creator = seed_user(&pool, "human").await;
    let sa = ServiceAccountRepositoryPg::new(pool.clone())
        .create(NewServiceAccount {
            organisation_id: org,
            name: "scoped-bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();
    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'org_admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(sa.user_id)
    .bind(org)
    .bind(role_id)
    .execute(&pool)
    .await
    .unwrap();

    let g = api_key_secret::generate().unwrap();
    ApiKeyRepositoryPg::new(pool.clone())
        .create(NewApiKeyRow {
            id: Uuid::now_v7(),
            service_account_user_id: sa.user_id,
            prefix: g.prefix.clone(),
            secret_hash: api_key_secret::hash_secret(&g.secret).unwrap(),
            name: "scoped".into(),
            scopes: Some(vec!["service_accounts.read".into()]),
            created_by: creator,
        })
        .await
        .unwrap();

    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/users?org_id={}", app.base_url, org))
        .bearer_auth(&g.plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    app.stop().await;
}
