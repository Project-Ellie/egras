use crate::common::seed::{grant_role, seed_org, seed_user};
use egras::security::service::list_users::{list_users, ListUsersError, ListUsersInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use uuid::Uuid;

#[tokio::test]
async fn operator_sees_all_users_with_all_memberships() {
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

    let out = list_users(
        &state,
        alice,
        /* is_operator = */ true,
        None, // caller_org_id irrelevant for operator
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 10,
        },
    )
    .await
    .expect("operator list_users");

    assert_eq!(out.items.len(), 2);
    let alice_item = out.items.iter().find(|u| u.id == alice).unwrap();
    assert_eq!(alice_item.memberships.len(), 1);
    assert!(out.next_cursor.is_none());
}

#[tokio::test]
async fn tenant_admin_sees_only_org_users_memberships_scoped_to_org() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let org1 = seed_org(&pool, "acme", "retail").await;
    let org2 = seed_org(&pool, "globex", "media").await;
    grant_role(&pool, alice, org1, "org_owner").await;
    grant_role(&pool, bob, org2, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = list_users(
        &state,
        alice,
        /* is_operator = */ false,
        Some(org1),
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 10,
        },
    )
    .await
    .expect("tenant admin list_users");

    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].id, alice);
    for item in &out.items {
        for m in &item.memberships {
            assert_eq!(m.org_id, org1);
        }
    }
}

#[tokio::test]
async fn invalid_cursor_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let err = list_users(
        &state,
        alice,
        true,
        None,
        ListUsersInput {
            org_id: None,
            q: None,
            after: Some("not-valid-base64!!".to_string()),
            limit: 10,
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, ListUsersError::InvalidCursor));
}

#[tokio::test]
async fn limit_clamped_to_100() {
    let pool = TestPool::fresh().await.pool;
    for i in 0..5 {
        seed_user(&pool, &format!("user{i}")).await;
    }

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = list_users(
        &state,
        Uuid::now_v7(),
        true,
        None,
        ListUsersInput {
            org_id: None,
            q: None,
            after: None,
            limit: 200, // above max — must be clamped
        },
    )
    .await
    .expect("clamped list");
    assert_eq!(out.items.len(), 5);
}
