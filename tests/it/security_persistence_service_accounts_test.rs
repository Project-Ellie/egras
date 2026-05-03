use egras::security::model::UserKind;
use egras::security::persistence::{
    NewServiceAccount, ServiceAccountRepoError, ServiceAccountRepository,
    ServiceAccountRepositoryPg, UserRepository, UserRepositoryPg,
};
use egras::testing::TestPool;

use crate::common::seed::{seed_org, seed_user};

#[tokio::test]
async fn create_then_find() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    let sa = repo
        .create(NewServiceAccount {
            organisation_id: org,
            name: "billing-bot".into(),
            description: Some("Bills runner".into()),
            created_by: creator,
        })
        .await
        .unwrap();

    let user = UserRepositoryPg::new(pool.clone())
        .find_by_id(sa.user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.kind, UserKind::ServiceAccount);

    let again = repo.find(org, sa.user_id).await.unwrap().unwrap();
    assert_eq!(again.name, "billing-bot");
    assert_eq!(again.description.as_deref(), Some("Bills runner"));
    assert_eq!(again.created_by, creator);
    assert!(again.last_used_at.is_none());
}

#[tokio::test]
async fn duplicate_name_in_org_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    repo.create(NewServiceAccount {
        organisation_id: org,
        name: "billing-bot".into(),
        description: None,
        created_by: creator,
    })
    .await
    .unwrap();

    let err = repo
        .create(NewServiceAccount {
            organisation_id: org,
            name: "billing-bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceAccountRepoError::DuplicateName));
}

#[tokio::test]
async fn duplicate_name_in_different_org_is_ok() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    repo.create(NewServiceAccount {
        organisation_id: org_a,
        name: "shared".into(),
        description: None,
        created_by: creator,
    })
    .await
    .unwrap();
    repo.create(NewServiceAccount {
        organisation_id: org_b,
        name: "shared".into(),
        description: None,
        created_by: creator,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn list_returns_only_org_sas_in_order() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    let sa1 = repo
        .create(NewServiceAccount {
            organisation_id: org_a,
            name: "z-bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();
    let sa2 = repo
        .create(NewServiceAccount {
            organisation_id: org_a,
            name: "a-bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();
    repo.create(NewServiceAccount {
        organisation_id: org_b,
        name: "outsider".into(),
        description: None,
        created_by: creator,
    })
    .await
    .unwrap();

    let listed = repo.list(org_a, 10, None).await.unwrap();
    assert_eq!(listed.len(), 2);
    // Ordering is by created_at ASC then user_id, so the first-inserted comes first.
    assert_eq!(listed[0].user_id, sa1.user_id);
    assert_eq!(listed[1].user_id, sa2.user_id);
}

#[tokio::test]
async fn delete_cascades_to_users_row() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());

    let sa = repo
        .create(NewServiceAccount {
            organisation_id: org,
            name: "bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();

    let removed = repo.delete(org, sa.user_id).await.unwrap();
    assert!(removed);

    let user = UserRepositoryPg::new(pool.clone())
        .find_by_id(sa.user_id)
        .await
        .unwrap();
    assert!(user.is_none(), "users row should cascade-delete");

    // Deleting again is a no-op.
    let removed_again = repo.delete(org, sa.user_id).await.unwrap();
    assert!(!removed_again);
}

#[tokio::test]
async fn delete_wrong_org_returns_false() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());
    let sa = repo
        .create(NewServiceAccount {
            organisation_id: org_a,
            name: "bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();

    assert!(!repo.delete(org_b, sa.user_id).await.unwrap());
    // SA still exists in its real org.
    assert!(repo.find(org_a, sa.user_id).await.unwrap().is_some());
}

#[tokio::test]
async fn touch_last_used_throttled() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let repo = ServiceAccountRepositoryPg::new(pool.clone());
    let sa = repo
        .create(NewServiceAccount {
            organisation_id: org,
            name: "bot".into(),
            description: None,
            created_by: creator,
        })
        .await
        .unwrap();

    repo.touch_last_used(sa.user_id).await.unwrap();
    let after_first = repo
        .find(org, sa.user_id)
        .await
        .unwrap()
        .unwrap()
        .last_used_at;
    assert!(after_first.is_some());

    repo.touch_last_used(sa.user_id).await.unwrap();
    let after_second = repo
        .find(org, sa.user_id)
        .await
        .unwrap()
        .unwrap()
        .last_used_at;
    assert_eq!(
        after_first, after_second,
        "second touch within 60 s must be a no-op"
    );
}
