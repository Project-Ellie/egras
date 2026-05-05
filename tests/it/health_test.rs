use egras::config::AppConfig;
use egras::testing::{TestApp, TestPool};

fn test_config() -> AppConfig {
    AppConfig {
        database_url: String::new(), // unused — we pass the pool directly
        database_max_connections: 5,
        bind_address: "127.0.0.1:0".into(),
        jwt_secret: "a".repeat(64),
        jwt_ttl_secs: 3600,
        jwt_issuer: "egras".into(),
        log_level: "info".into(),
        log_format: "json".into(),
        cors_allowed_origins: "http://localhost:3000".into(),
        password_reset_ttl_secs: 3600,
        operator_org_name: "operator".into(),
        audit_channel_capacity: 32,
        audit_max_retries: 3,
        audit_retry_backoff_ms_initial: 10,
    }
}

#[tokio::test]
async fn health_returns_ok() {
    let tp = TestPool::fresh().await;
    let app = TestApp::spawn(tp.pool.clone(), test_config()).await;

    let resp = reqwest::get(format!("{}/health", app.base_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(v["status"], "ok");

    app.stop().await;
}

#[tokio::test]
async fn cors_wildcard_origin_starts_app() {
    // Regression: tower-http panics if "*" appears in an origin list. Server start
    // must accept the wildcard form by promoting it to AllowOrigin::any().
    let tp = TestPool::fresh().await;
    let mut cfg = test_config();
    cfg.cors_allowed_origins = "*".into();
    let app = TestApp::spawn(tp.pool.clone(), cfg).await;

    let resp = reqwest::get(format!("{}/health", app.base_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    app.stop().await;
}

#[tokio::test]
async fn ready_returns_ok_when_db_reachable() {
    let tp = TestPool::fresh().await;
    let app = TestApp::spawn(tp.pool.clone(), test_config()).await;

    let resp = reqwest::get(format!("{}/ready", app.base_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(v["status"], "ready");

    app.stop().await;
}

#[tokio::test]
async fn migration_0005_seeded_operator_org() {
    let tp = TestPool::fresh().await;
    let row: (String,) = sqlx::query_as("SELECT name FROM organisations WHERE is_operator = TRUE")
        .fetch_one(&tp.pool)
        .await
        .unwrap();
    assert_eq!(row.0, "operator");
}

#[tokio::test]
async fn migration_0005_has_all_built_in_roles() {
    let tp = TestPool::fresh().await;
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT code FROM roles WHERE is_builtin = TRUE ORDER BY code")
            .fetch_all(&tp.pool)
            .await
            .unwrap();
    let codes: Vec<String> = rows.into_iter().map(|r| r.0).collect();
    assert_eq!(
        codes,
        vec!["operator_admin", "org_admin", "org_member", "org_owner"]
    );
}
