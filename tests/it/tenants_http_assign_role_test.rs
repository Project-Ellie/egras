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
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/memberships",
            app.base_url
        ))
        .json(&json!({ "user_id": uuid::Uuid::new_v4(), "role_code": "org_admin" }))
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
async fn missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    // org_member does NOT have tenants.roles.assign (confirmed in migration 0005)
    grant_role(&pool, actor, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, actor, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/memberships",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({ "user_id": uuid::Uuid::new_v4(), "role_code": "org_admin" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "https://egras.dev/errors/permission.denied");
    app.stop().await;
}

#[tokio::test]
async fn happy_path_assigns_role_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, actor, org);
    let app = TestApp::spawn(pool.clone(), cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/memberships",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({ "user_id": target, "role_code": "org_admin" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["assigned"], true);

    app.stop().await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events \
         WHERE event_type = 'organisation.role_assigned' \
           AND actor_user_id = $1 \
           AND target_id = $2 \
           AND target_organisation_id = $3",
    )
    .bind(actor)
    .bind(target)
    .bind(org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn unknown_role_code_returns_400_validation_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let target = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, actor, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/organisations/{org}/memberships",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({ "user_id": target, "role_code": "no_such" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/validation.invalid_request"
    );
    assert_eq!(body["errors"]["role_code"][0], "unknown_role_code");
    app.stop().await;
}
