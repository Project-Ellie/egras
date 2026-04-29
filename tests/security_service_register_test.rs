#[path = "common/mod.rs"]
mod common;

use common::seed::{seed_org, seed_user};
use egras::security::service::register_user::{
    register_user, RegisterUserError, RegisterUserInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

#[tokio::test]
async fn register_happy_path_creates_user_and_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin").await;
    let org = seed_org(&pool, "acme", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let out = register_user(
        &state,
        actor,
        org,
        RegisterUserInput {
            username: "newuser".into(),
            email: "newuser@example.com".into(),
            password: "password123".into(),
            target_org_id: org,
            role_code: "org_member".into(),
        },
    )
    .await
    .expect("register");

    let user = state.users.find_by_id(out.user_id).await.unwrap().unwrap();
    assert_eq!(user.username, "newuser");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'user.registered'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn register_duplicate_username_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin2").await;
    let org = seed_org(&pool, "acme2", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let input = || RegisterUserInput {
        username: "dupuser".into(),
        email: "dup@example.com".into(),
        password: "password123".into(),
        target_org_id: org,
        role_code: "org_member".into(),
    };

    register_user(&state, actor, org, input()).await.unwrap();

    let err = register_user(
        &state,
        actor,
        org,
        RegisterUserInput {
            email: "other@example.com".into(),
            ..input()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RegisterUserError::DuplicateUsername));
}
