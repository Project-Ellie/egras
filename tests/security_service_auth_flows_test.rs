#[path = "common/mod.rs"]
mod common;

use chrono::Utc;
use common::seed::{grant_role, seed_org, seed_user, seed_user_with_password};
use egras::security::service::change_password::{
    change_password, ChangePasswordError, ChangePasswordInput,
};
use egras::security::service::logout::logout;
use egras::security::service::switch_org::{switch_org, SwitchOrgError, SwitchOrgInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use uuid::Uuid;

#[tokio::test]
async fn logout_emits_audit_event() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "audit-user").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let org = Uuid::now_v7();
    let jti = Uuid::now_v7();
    let expires_at = Utc::now() + chrono::Duration::hours(1);

    logout(&state, user, org, jti, expires_at).await.unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE event_type = 'logout'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn logout_revokes_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "revoke-user").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let org = Uuid::now_v7();
    let jti = Uuid::now_v7();
    let expires_at = Utc::now() + chrono::Duration::hours(1);

    assert!(!state.tokens.is_revoked(jti).await.unwrap());

    logout(&state, user, org, jti, expires_at).await.unwrap();

    assert!(state.tokens.is_revoked(jti).await.unwrap());
}

#[tokio::test]
async fn change_password_wrong_current_is_error() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "carol", "original").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = change_password(
        &state,
        user,
        ChangePasswordInput {
            current_password: "wrong".into(),
            new_password: "newpassword1".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ChangePasswordError::WrongCurrentPassword));
}

#[tokio::test]
async fn change_password_happy_path_updates_hash() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "dave", "original").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    change_password(
        &state,
        user,
        ChangePasswordInput {
            current_password: "original".into(),
            new_password: "newpassword1".into(),
        },
    )
    .await
    .unwrap();

    let updated = state.users.find_by_id(user).await.unwrap().unwrap();
    assert!(egras::security::service::password_hash::verify_password(
        "newpassword1",
        &updated.password_hash
    )
    .unwrap());
}

#[tokio::test]
async fn switch_org_not_member_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "eve", "pass1234").await;
    let home_org = seed_org(&pool, "eve-home", "retail").await;
    let other_org = seed_org(&pool, "other-org", "media").await;
    grant_role(&pool, user, home_org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = switch_org(
        &state,
        user,
        home_org,
        SwitchOrgInput {
            target_org_id: other_org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SwitchOrgError::NotMember));
}

#[tokio::test]
async fn switch_org_happy_path_returns_new_token() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user_with_password(&pool, "frank", "pass1234").await;
    let org1 = seed_org(&pool, "frank-org1", "retail").await;
    let org2 = seed_org(&pool, "frank-org2", "media").await;
    grant_role(&pool, user, org1, "org_member").await;
    grant_role(&pool, user, org2, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = switch_org(
        &state,
        user,
        org1,
        SwitchOrgInput {
            target_org_id: org2,
        },
    )
    .await
    .unwrap();

    assert!(!out.token.is_empty());
}
