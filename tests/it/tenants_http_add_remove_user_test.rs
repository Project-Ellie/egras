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
async fn add_user_unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let target = seed_user(&pool, "add_target_unauth").await;
    let org = seed_org(&pool, "add-unauth-org", "retail").await;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/add-user-to-organisation",
            app.base_url
        ))
        .json(&json!({
            "user_id": target,
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
async fn add_user_missing_permission_returns_403() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "add_caller_no_perm").await;
    let target = seed_user(&pool, "add_target_no_perm").await;
    let org = seed_org(&pool, "add-no-perm-org", "retail").await;
    grant_role(&pool, caller, org, "org_member").await; // lacks tenants.members.add

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/add-user-to-organisation",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({
            "user_id": target,
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
async fn add_user_happy_path_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "add_caller_owner").await;
    let target = seed_user(&pool, "add_target_new").await;
    let org = seed_org(&pool, "add-owner-org", "retail").await;
    grant_role(&pool, caller, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/add-user-to-organisation",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({
            "user_id": target,
            "org_id": org,
            "role_code": "org_member"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn remove_user_happy_path_returns_204() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "rem_caller").await;
    let owner = seed_user(&pool, "rem_owner").await;
    let member = seed_user(&pool, "rem_member").await;
    let org = seed_org(&pool, "rem-org", "retail").await;
    grant_role(&pool, caller, org, "org_owner").await;
    grant_role(&pool, owner, org, "org_owner").await;
    grant_role(&pool, member, org, "org_member").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/remove-user-from-organisation",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({
            "user_id": member,
            "org_id": org
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    app.stop().await;
}

#[tokio::test]
async fn remove_last_owner_returns_409() {
    let pool = TestPool::fresh().await.pool;
    let caller = seed_user(&pool, "rem_sole_owner").await;
    let org = seed_org(&pool, "rem-sole-org", "retail").await;
    grant_role(&pool, caller, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, caller, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/tenants/remove-user-from-organisation",
            app.base_url
        ))
        .header("authorization", token)
        .json(&json!({
            "user_id": caller,
            "org_id": org
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    app.stop().await;
}
