use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};
use reqwest::StatusCode;

use crate::common::auth::bearer;
use crate::common::seed::{grant_role, seed_org, seed_user};

fn test_config() -> AppConfig {
    AppConfig::default_for_tests()
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool, test_config()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/tenants/me/organisations", app.base_url))
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
async fn caller_sees_only_their_own_orgs() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let a1 = seed_org(&pool, "alice-1", "retail").await;
    let a2 = seed_org(&pool, "alice-2", "retail").await;
    let b_only = seed_org(&pool, "bob-only", "retail").await;
    grant_role(&pool, alice, a1, "org_owner").await;
    grant_role(&pool, alice, a2, "org_member").await;
    grant_role(&pool, bob, b_only, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, a1);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/tenants/me/organisations", app.base_url))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let names: Vec<String> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|n| n.starts_with("alice-")));
    app.stop().await;
}

#[tokio::test]
async fn happy_path_paginates_with_cursor() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let mut first_org = None;
    for i in 0..3 {
        let o = seed_org(&pool, &format!("o-{i}"), "retail").await;
        grant_role(&pool, alice, o, "org_owner").await;
        if first_org.is_none() {
            first_org = Some(o);
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, first_org.unwrap());
    let app = TestApp::spawn(pool, cfg).await;

    let resp1 = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/me/organisations?limit=2",
            app.base_url
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1: serde_json::Value = resp1.json().await.unwrap();
    assert_eq!(body1["items"].as_array().unwrap().len(), 2);
    let cursor = body1["next_cursor"]
        .as_str()
        .expect("next_cursor is present")
        .to_string();

    let resp2 = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/me/organisations?limit=2&after={}",
            app.base_url, cursor
        ))
        .header("authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2: serde_json::Value = resp2.json().await.unwrap();
    assert_eq!(body2["items"].as_array().unwrap().len(), 1);
    assert!(body2["next_cursor"].is_null());
    app.stop().await;
}

#[tokio::test]
async fn invalid_cursor_returns_400() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let cfg = test_config();
    let token = bearer(&cfg.jwt_secret, &cfg.jwt_issuer, alice, org);
    let app = TestApp::spawn(pool, cfg).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/tenants/me/organisations?after=not-a-real-cursor",
            app.base_url
        ))
        .header("authorization", token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["type"],
        "https://egras.dev/errors/validation.invalid_request"
    );
    app.stop().await;
}
