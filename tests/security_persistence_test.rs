#[path = "common/mod.rs"]
mod common;

use common::seed::{grant_role, seed_org, seed_user};
use egras::testing::{MockAppStateBuilder, TestPool};

#[tokio::test]
async fn user_repository_create_and_find() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let user = state
        .users
        .create("testuser", "testuser@example.com", "hash")
        .await
        .expect("create user");

    assert_eq!(user.username, "testuser");

    let found = state
        .users
        .find_by_username_or_email("testuser")
        .await
        .expect("find by username")
        .expect("should exist");
    assert_eq!(found.id, user.id);
}

#[tokio::test]
async fn user_repository_duplicate_username_is_error() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    state
        .users
        .create("dupe", "dupe1@example.com", "h")
        .await
        .unwrap();
    let err = state
        .users
        .create("dupe", "dupe2@example.com", "h")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        egras::security::persistence::UserRepoError::DuplicateUsername(_)
    ));
}

#[tokio::test]
async fn list_memberships_returns_orgs() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "memuser").await;
    let org = seed_org(&pool, "memorg", "retail").await;
    grant_role(&pool, user, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let memberships = state.users.list_memberships(user).await.expect("list");
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0].org_id, org);
}

#[tokio::test]
async fn token_repository_insert_find_consume() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "tokuser").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(900);
    let tok = state
        .tokens
        .insert(user, "deadbeef_hash", expires_at)
        .await
        .expect("insert");

    let found = state
        .tokens
        .find_valid("deadbeef_hash")
        .await
        .expect("find")
        .expect("should exist");
    assert_eq!(found.id, tok.id);

    state.tokens.consume(tok.id).await.expect("consume");

    let gone = state
        .tokens
        .find_valid("deadbeef_hash")
        .await
        .expect("find after consume");
    assert!(gone.is_none());
}

#[tokio::test]
async fn token_repository_drops_oldest_when_at_capacity() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "capuser").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let exp = chrono::Utc::now() + chrono::Duration::seconds(900);
    for i in 0..3i32 {
        state
            .tokens
            .insert(user, &format!("hash_{i}"), exp)
            .await
            .expect("insert");
    }
    state
        .tokens
        .insert(user, "hash_new", exp)
        .await
        .expect("4th insert");

    assert!(state.tokens.find_valid("hash_0").await.unwrap().is_none());
    assert!(state.tokens.find_valid("hash_new").await.unwrap().is_some());

    // hash_1 and hash_2 (not oldest) should still be valid.
    assert!(state.tokens.find_valid("hash_1").await.unwrap().is_some());
    assert!(state.tokens.find_valid("hash_2").await.unwrap().is_some());
}
