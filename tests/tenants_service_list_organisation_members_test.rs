#[path = "common/mod.rs"]
mod common;

use egras::tenants::service::list_organisation_members::{
    list_organisation_members, ListMembersError, ListMembersInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn list_organisation_members_happy_path() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;
    grant_role(&pool, bob, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let page = list_organisation_members(
        &state,
        alice,
        /* is_operator = */ false,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 2);
}

#[tokio::test]
async fn non_member_without_manage_all_gets_not_found() {
    let pool = TestPool::fresh().await.pool;
    let mallory = seed_user(&pool, "mallory").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = list_organisation_members(
        &state,
        mallory,
        false,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ListMembersError::NotFound));
}

#[tokio::test]
async fn operator_bypass_sees_non_member_org() {
    let pool = TestPool::fresh().await.pool;
    let op = seed_user(&pool, "op").await;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let page = list_organisation_members(
        &state,
        op,
        /* is_operator = */ true,
        ListMembersInput {
            organisation_id: org,
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
}
