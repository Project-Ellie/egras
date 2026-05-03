use crate::common::seed::{grant_role, seed_org, seed_user_with_password};
use egras::security::service::login::{login, LoginError, LoginInput};
use egras::testing::{MockAppStateBuilder, TestPool};

#[tokio::test]
async fn login_happy_path_returns_token_and_memberships() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "alice", "hunter2").await;
    let org = seed_org(&pool, "alice-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = login(
        &state,
        LoginInput {
            username_or_email: "alice".into(),
            password: "hunter2".into(),
        },
    )
    .await
    .expect("login");

    assert!(!out.token.is_empty());
    assert_eq!(out.user_id, user);
    assert_eq!(out.memberships.len(), 1);
    assert_eq!(out.memberships[0].org_id, org);
}

#[tokio::test]
async fn login_wrong_password_returns_invalid_credentials() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "bob", "correct").await;
    let org = seed_org(&pool, "bob-org", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = login(
        &state,
        LoginInput {
            username_or_email: "bob".into(),
            password: "wrong".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LoginError::InvalidCredentials));
}

#[tokio::test]
async fn login_unknown_user_returns_invalid_credentials() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = login(
        &state,
        LoginInput {
            username_or_email: "nobody".into(),
            password: "x".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LoginError::InvalidCredentials));
}
