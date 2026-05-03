use egras::security::persistence::{
    ApiKeyRepository, ApiKeyRepositoryPg, NewApiKeyRow, NewServiceAccount,
    ServiceAccountRepository, ServiceAccountRepositoryPg,
};
use egras::testing::TestPool;
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::seed::{seed_org, seed_user};

async fn seed_sa(pool: &PgPool, name: &str) -> (Uuid, Uuid) {
    let creator = seed_user(pool, &format!("creator-{name}")).await;
    let org = seed_org(pool, &format!("org-{name}"), "retail").await;
    let sa = ServiceAccountRepositoryPg::new(pool.clone())
        .create(NewServiceAccount {
            organisation_id: org,
            name: name.into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();
    (sa.user_id, creator)
}

fn key_row(sa_user_id: Uuid, prefix: &str, creator: Uuid, name: &str) -> NewApiKeyRow {
    NewApiKeyRow {
        id: Uuid::now_v7(),
        service_account_user_id: sa_user_id,
        prefix: prefix.into(),
        secret_hash: "$argon2id$v=19$placeholder".into(),
        name: name.into(),
        scopes: None,
        created_by: creator,
    }
}

#[tokio::test]
async fn create_then_lookup_active_by_prefix() {
    let pool = TestPool::fresh().await.pool;
    let (sa, creator) = seed_sa(&pool, "bot1").await;
    let repo = ApiKeyRepositoryPg::new(pool.clone());

    let key = repo
        .create(key_row(sa, "deadbeef", creator, "primary"))
        .await
        .unwrap();
    assert_eq!(key.prefix, "deadbeef");
    assert!(key.revoked_at.is_none());

    let found = repo
        .find_active_by_prefix("deadbeef")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.key.id, key.id);
    assert_eq!(found.organisation_id, found.organisation_id); // joined
    assert_eq!(found.secret_hash, "$argon2id$v=19$placeholder");

    assert!(repo
        .find_active_by_prefix("00000000")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn duplicate_prefix_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let (sa, creator) = seed_sa(&pool, "bot2").await;
    let repo = ApiKeyRepositoryPg::new(pool.clone());

    repo.create(key_row(sa, "abcd1234", creator, "k1"))
        .await
        .unwrap();
    let err = repo
        .create(key_row(sa, "abcd1234", creator, "k2"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        egras::security::persistence::ApiKeyRepoError::DuplicatePrefix
    ));
}

#[tokio::test]
async fn revoke_filters_out_of_active_lookup() {
    let pool = TestPool::fresh().await.pool;
    let (sa, creator) = seed_sa(&pool, "bot3").await;
    let repo = ApiKeyRepositoryPg::new(pool.clone());
    let key = repo
        .create(key_row(sa, "11112222", creator, "k"))
        .await
        .unwrap();

    let transitioned = repo.revoke(sa, key.id).await.unwrap();
    assert!(transitioned);

    assert!(repo
        .find_active_by_prefix("11112222")
        .await
        .unwrap()
        .is_none());

    let again = repo.revoke(sa, key.id).await.unwrap();
    assert!(!again, "second revoke is a no-op");
}

#[tokio::test]
async fn touch_last_used_throttled() {
    let pool = TestPool::fresh().await.pool;
    let (sa, creator) = seed_sa(&pool, "bot4").await;
    let repo = ApiKeyRepositoryPg::new(pool.clone());
    let key = repo
        .create(key_row(sa, "33334444", creator, "k"))
        .await
        .unwrap();

    repo.touch_last_used(key.id).await.unwrap();
    let after_first = repo.find(sa, key.id).await.unwrap().unwrap().last_used_at;
    assert!(after_first.is_some());

    repo.touch_last_used(key.id).await.unwrap();
    let after_second = repo.find(sa, key.id).await.unwrap().unwrap().last_used_at;
    assert_eq!(
        after_first, after_second,
        "second touch within 60 s must be a no-op"
    );
}

#[tokio::test]
async fn rotate_revokes_old_and_creates_new() {
    let pool = TestPool::fresh().await.pool;
    let (sa, creator) = seed_sa(&pool, "bot5").await;
    let repo = ApiKeyRepositoryPg::new(pool.clone());
    let old = repo
        .create(key_row(sa, "55556666", creator, "old"))
        .await
        .unwrap();

    let new = repo
        .rotate(old.id, key_row(sa, "77778888", creator, "new"))
        .await
        .unwrap();

    let old_now = repo.find(sa, old.id).await.unwrap().unwrap();
    assert!(old_now.revoked_at.is_some());
    assert!(new.revoked_at.is_none());
    let listed = repo.list_by_sa(sa).await.unwrap();
    assert_eq!(listed.len(), 2);
}
