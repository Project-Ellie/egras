use egras::tenants::service::assign_role::{assign_role, AssignRoleError, AssignRoleInput};
use egras::testing::{MockAppStateBuilder, TestPool};

use crate::common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn assign_role_happy_path_was_new_true() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "actor").await;
    let target = seed_user(&pool, "target").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = assign_role(
        &state,
        actor,
        org,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_admin".into(),
        },
    )
    .await
    .unwrap();

    assert!(out.was_new);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = $1 AND target_id = $2 AND target_organisation_id = $3",
    )
    .bind("organisation.role_assigned")
    .bind(target)
    .bind(org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "exactly one audit event should fire on fresh assign"
    );
}

#[tokio::test]
async fn assign_role_idempotent_was_new_false() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "actor").await;
    let target = seed_user(&pool, "target").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_admin").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    // Re-assign the same role that is already held.
    let out = assign_role(
        &state,
        actor,
        org,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_admin".into(),
        },
    )
    .await
    .unwrap();

    assert!(!out.was_new);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = $1 AND target_id = $2 AND target_organisation_id = $3",
    )
    .bind("organisation.role_assigned")
    .bind(target)
    .bind(org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "no audit event should fire on idempotent repeat assign"
    );
}

#[tokio::test]
async fn assign_role_unknown_role_code_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "actor").await;
    let target = seed_user(&pool, "target").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, actor, org, "org_owner").await;
    grant_role(&pool, target, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = assign_role(
        &state,
        actor,
        org,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "no_such_role".into(),
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AssignRoleError::UnknownRoleCode));
}

#[tokio::test]
async fn assign_role_non_member_actor_gets_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "actor").await;
    let target = seed_user(&pool, "target").await;
    let org = seed_org(&pool, "acme", "retail").await;
    // actor is NOT a member of org
    grant_role(&pool, target, org, "org_member").await;

    // actor_org can be any UUID since cross-org check short-circuits before audit.
    let other_org = seed_org(&pool, "other", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = assign_role(
        &state,
        actor,
        other_org,
        /* is_operator = */ false,
        AssignRoleInput {
            organisation_id: org,
            target_user_id: target,
            role_code: "org_admin".into(),
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AssignRoleError::NotFound));
}
