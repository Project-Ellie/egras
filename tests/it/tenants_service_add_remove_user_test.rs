use crate::common::seed::{grant_role, seed_org, seed_user};
use egras::tenants::service::add_user_to_organisation::{
    add_user_to_organisation, AddUserToOrganisationInput,
};
use egras::tenants::service::remove_user_from_organisation::{
    remove_user_from_organisation, RemoveUserFromOrganisationError, RemoveUserFromOrganisationInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

#[tokio::test]
async fn add_user_to_organisation_happy_path() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_add").await;
    let target = seed_user(&pool, "newbie_add").await;
    let org = seed_org(&pool, "org-add", "retail").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    add_user_to_organisation(
        &state,
        actor,
        org,
        AddUserToOrganisationInput {
            user_id: target,
            org_id: org,
            role_code: "org_member".into(),
        },
    )
    .await
    .expect("add user");

    let is_member = state.organisations.is_member(target, org).await.unwrap();
    assert!(is_member);
}

#[tokio::test]
async fn remove_last_owner_returns_last_owner_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_rem").await;
    let owner = seed_user(&pool, "owner_rem").await;
    let org = seed_org(&pool, "org-rem", "retail").await;
    grant_role(&pool, owner, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = remove_user_from_organisation(
        &state,
        actor,
        org,
        RemoveUserFromOrganisationInput {
            user_id: owner,
            org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RemoveUserFromOrganisationError::LastOwner));
}

#[tokio::test]
async fn remove_non_owner_member_succeeds() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "admin_rem2").await;
    let owner = seed_user(&pool, "owner_rem2").await;
    let member = seed_user(&pool, "member_rem2").await;
    let org = seed_org(&pool, "org-rem2", "retail").await;
    grant_role(&pool, owner, org, "org_owner").await;
    grant_role(&pool, member, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    remove_user_from_organisation(
        &state,
        actor,
        org,
        RemoveUserFromOrganisationInput {
            user_id: member,
            org_id: org,
        },
    )
    .await
    .expect("remove member");

    let is_member = state.organisations.is_member(member, org).await.unwrap();
    assert!(!is_member);
}
