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

#[tokio::test]
async fn create_returns_plaintext_then_use_key_to_authenticate() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let sa = make_sa(&cli, &app.base_url, &auth, org, "bot").await;

    // POST api-key
    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .json(&json!({"name": "primary", "scopes": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();

    // Grant the SA tenants.members.list via a direct insert (assigning roles
    // to SAs via the public API requires its own membership bootstrap; the
    // key auth path is what we are testing here).
    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'org_admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(sa)
    .bind(org)
    .bind(role_id)
    .execute(&pool)
    .await
    .unwrap();

    // Use the plaintext key on a protected endpoint.
    let resp = cli
        .get(format!("{}/api/v1/users?org_id={}", app.base_url, org))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // GET keys returns metadata only; no plaintext field.
    let resp = cli
        .get(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let item = &body["items"][0];
    assert!(item.get("plaintext").is_none());
    assert!(item.get("secret_hash").is_none());

    app.stop().await;
}

#[tokio::test]
async fn revoke_then_use_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let sa = make_sa(&cli, &app.base_url, &auth, org, "bot").await;
    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .json(&json!({"name": "k", "scopes": null}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();
    let key_id = Uuid::parse_str(body["key"]["id"].as_str().unwrap()).unwrap();

    // DELETE the key.
    let resp = cli
        .delete(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys/{}",
            app.base_url, sa, key_id
        ))
        .header("authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Use the revoked key.
    let resp = cli
        .get(format!("{}/api/v1/users", app.base_url))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}

#[tokio::test]
async fn rotate_returns_new_plaintext_old_no_longer_works() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let sa = make_sa(&cli, &app.base_url, &auth, org, "bot").await;
    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .json(&json!({"name": "k", "scopes": null}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let old_plaintext = body["plaintext"].as_str().unwrap().to_string();
    let old_id = Uuid::parse_str(body["key"]["id"].as_str().unwrap()).unwrap();

    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys/{}/rotate",
            app.base_url, sa, old_id
        ))
        .header("authorization", &auth)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let new_plaintext = body["plaintext"].as_str().unwrap();
    assert_ne!(new_plaintext, old_plaintext);

    // Old key fails.
    let resp = cli
        .get(format!("{}/api/v1/users", app.base_url))
        .bearer_auth(&old_plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    app.stop().await;
}

#[tokio::test]
async fn api_key_caller_cannot_logout() {
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let sa = make_sa(&cli, &app.base_url, &auth, org, "bot").await;
    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .json(&json!({"name": "k", "scopes": null}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();

    let resp = cli
        .post(format!("{}/api/v1/security/logout", app.base_url))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/auth.requires_user_credentials"
    );

    app.stop().await;
}

#[tokio::test]
async fn api_key_caller_cannot_create_another_service_account() {
    // Pivot-escalation guard: a stolen key cannot mint more keys / SAs.
    let pool = TestPool::fresh().await.pool;
    let (_user, org, auth) = human_admin(&pool).await;
    let app = TestApp::spawn(pool.clone(), test_config()).await;
    let cli = reqwest::Client::new();

    let sa = make_sa(&cli, &app.base_url, &auth, org, "bot").await;
    let resp = cli
        .post(format!(
            "{}/api/v1/security/service-accounts/{}/api-keys",
            app.base_url, sa
        ))
        .header("authorization", &auth)
        .json(&json!({"name": "k", "scopes": null}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();

    // Grant the SA org_admin so it has perms — but `RequireHumanCaller`
    // should still reject SA-management ops regardless of permissions.
    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'org_admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(sa)
    .bind(org)
    .bind(role_id)
    .execute(&pool)
    .await
    .unwrap();

    let resp = cli
        .post(format!("{}/api/v1/security/service-accounts", app.base_url))
        .bearer_auth(&plaintext)
        .json(&json!({"organisation_id": org, "name": "another-bot", "description": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/auth.requires_user_credentials"
    );

    app.stop().await;
}
