#[path = "common/mod.rs"]
mod common;

use common::seed::{grant_role, seed_org, seed_user};
use egras::security::model::UserCursor;
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

#[tokio::test]
async fn list_users_returns_all_platform_users() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(None, None, None, 10)
        .await
        .expect("list_users");
    assert_eq!(users.len(), 2);
}

#[tokio::test]
async fn list_users_filtered_by_org_id() {
    let pool = TestPool::fresh().await.pool;
    let alice = seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await; // not in the org
    let org = seed_org(&pool, "acme", "retail").await;
    grant_role(&pool, alice, org, "org_member").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(Some(org), None, None, 10)
        .await
        .expect("list_users filtered");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, alice);
}

#[tokio::test]
async fn list_users_search_by_username() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await;
    seed_user(&pool, "bob").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let users = state
        .users
        .list_users(None, Some("ali"), None, 10)
        .await
        .expect("list_users q=ali");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn list_users_search_by_email() {
    let pool = TestPool::fresh().await.pool;
    seed_user(&pool, "alice").await; // email: alice@test
    seed_user(&pool, "bob").await;   // email: bob@test

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // seed_user creates email as "{username}@test"
    let users = state
        .users
        .list_users(None, Some("alice@test"), None, 10)
        .await
        .expect("list_users q=email");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn list_users_cursor_pagination() {
    let pool = TestPool::fresh().await.pool;
    // Insert 3 users; use cursor from second to get only third.
    let _u1 = seed_user(&pool, "user_a").await;
    let _u2 = seed_user(&pool, "user_b").await;
    let u3 = seed_user(&pool, "user_c").await;

    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    // Fetch all 3 to get stable ordering.
    let all = state
        .users
        .list_users(None, None, None, 10)
        .await
        .expect("all users");
    assert_eq!(all.len(), 3);

    // Use the second user as the cursor boundary.
    let cursor = UserCursor {
        created_at: all[1].created_at,
        user_id: all[1].id,
    };
    let page2 = state
        .users
        .list_users(None, None, Some(cursor), 10)
        .await
        .expect("page2");
    // Only user_c should be after user_b.
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].id, u3);
}

#[tokio::test]
async fn list_memberships_for_users_batch() {
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
        .with_pg_security_repos()
        .build();

    let memberships = state
        .users
        .list_memberships_for_users(&[alice, bob])
        .await
        .expect("batch memberships");

    assert_eq!(memberships.len(), 2);
    let alice_m: Vec<_> = memberships.iter().filter(|(uid, _)| *uid == alice).collect();
    let bob_m: Vec<_> = memberships.iter().filter(|(uid, _)| *uid == bob).collect();
    assert_eq!(alice_m.len(), 1);
    assert_eq!(alice_m[0].1.org_id, org1);
    assert_eq!(bob_m.len(), 1);
    assert_eq!(bob_m[0].1.org_id, org2);
}
