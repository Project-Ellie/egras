#[path = "common/mod.rs"]
mod common;

use egras::tenants::service::list_my_organisations::{
    list_my_organisations, ListMyOrganisationsInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

use common::seed::{grant_role, seed_org, seed_user};

#[tokio::test]
async fn list_my_organisations_returns_only_caller_orgs() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    let o1 = seed_org(&pool, "alice-1", "retail").await;
    let o2 = seed_org(&pool, "alice-2", "retail").await;
    let ohidden = seed_org(&pool, "bob-only", "retail").await;

    grant_role(&pool, alice, o1, "org_owner").await;
    grant_role(&pool, alice, o2, "org_member").await;
    grant_role(&pool, bob, ohidden, "org_owner").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let page = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: None,
            limit: 50,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.items.iter().all(|o| o.name.starts_with("alice-")));
    assert!(page.next_cursor.is_none());
}

#[tokio::test]
async fn list_my_organisations_paginates() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    for i in 0..3 {
        let o = seed_org(&pool, &format!("o-{i}"), "retail").await;
        grant_role(&pool, alice, o, "org_owner").await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let page1 = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: None,
            limit: 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.next_cursor.is_some());

    let page2 = list_my_organisations(
        &state,
        alice,
        ListMyOrganisationsInput {
            after: page1.next_cursor,
            limit: 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(page2.next_cursor.is_none());
}
