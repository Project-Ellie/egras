use egras::tenants::model::ChannelType;
use egras::tenants::persistence::channel_repository::{ChannelRepoError, InboundChannelRepository};
use egras::tenants::persistence::channel_repository_pg::InboundChannelRepositoryPg;
use egras::testing::TestPool;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_org(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO organisations (id, name, business, is_operator) VALUES ($1, $2, 'test', FALSE)",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await
    .expect("seed org");
    id
}

#[tokio::test]
async fn create_returns_channel_with_generated_api_key() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo
        .create(org, "VAST Feed", Some("main VAST feed"), ChannelType::Vast, true)
        .await
        .unwrap();

    assert_eq!(ch.organisation_id, org);
    assert_eq!(ch.name, "VAST Feed");
    assert_eq!(ch.description, Some("main VAST feed".into()));
    assert_eq!(ch.channel_type, ChannelType::Vast);
    assert!(ch.is_active);
    assert_eq!(ch.api_key.len(), 64);
}

#[tokio::test]
async fn create_duplicate_name_in_same_org_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "acme2").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.create(org, "feed", None, ChannelType::Sensor, true).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::DuplicateName(n) if n == "feed"));
}

#[tokio::test]
async fn duplicate_name_in_different_org_is_allowed() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-a").await;
    let org2 = seed_org(&pool, "org-b").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    repo.create(org2, "feed", None, ChannelType::Rest, true).await.unwrap();
}

#[tokio::test]
async fn get_returns_not_found_for_wrong_org() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-c").await;
    let org2 = seed_org(&pool, "org-d").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.get(org2, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}

#[tokio::test]
async fn list_returns_channels_for_org_only() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-e").await;
    let org2 = seed_org(&pool, "org-f").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org1, "feed-a", None, ChannelType::Vast, true).await.unwrap();
    repo.create(org1, "feed-b", None, ChannelType::Sensor, true).await.unwrap();
    repo.create(org2, "feed-x", None, ChannelType::Rest, true).await.unwrap();

    let items = repo.list(org1, None, 50).await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|c| c.organisation_id == org1));
}

#[tokio::test]
async fn update_changes_mutable_fields_and_preserves_api_key() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "org-g").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org, "old-name", None, ChannelType::Vast, true).await.unwrap();
    let original_key = ch.api_key.clone();

    let updated = repo
        .update(org, ch.id, "new-name", Some("desc"), ChannelType::Sensor, false)
        .await
        .unwrap();

    assert_eq!(updated.name, "new-name");
    assert_eq!(updated.description, Some("desc".into()));
    assert_eq!(updated.channel_type, ChannelType::Sensor);
    assert!(!updated.is_active);
    assert_eq!(updated.api_key, original_key);
    assert!(updated.updated_at >= ch.updated_at);
}

#[tokio::test]
async fn delete_removes_channel_and_get_returns_not_found_after() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "org-h").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org, "feed", None, ChannelType::Rest, true).await.unwrap();
    repo.delete(org, ch.id).await.unwrap();
    let err = repo.get(org, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}

#[tokio::test]
async fn delete_wrong_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "org-i").await;
    let org2 = seed_org(&pool, "org-j").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    let ch = repo.create(org1, "feed", None, ChannelType::Rest, true).await.unwrap();
    let err = repo.delete(org2, ch.id).await.unwrap_err();
    assert!(matches!(err, ChannelRepoError::NotFound));
}

#[tokio::test]
async fn list_cursor_pagination_returns_only_remaining_items() {
    use egras::tenants::model::ChannelCursor;

    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "org-k").await;
    let repo = InboundChannelRepositoryPg::new(pool);

    repo.create(org, "feed-1", None, ChannelType::Vast, true).await.unwrap();
    repo.create(org, "feed-2", None, ChannelType::Sensor, true).await.unwrap();
    repo.create(org, "feed-3", None, ChannelType::Rest, true).await.unwrap();

    // First page: limit 2 (returns newest first)
    let first_page = repo.list(org, None, 2).await.unwrap();
    assert_eq!(first_page.len(), 2);

    // Build cursor from last item of first page
    let cursor = ChannelCursor {
        created_at: first_page[1].created_at,
        id: first_page[1].id,
    };

    // Second page: should return the one remaining item
    let second_page = repo.list(org, Some(cursor), 10).await.unwrap();
    assert_eq!(second_page.len(), 1);

    // Items from both pages must be distinct
    assert!(second_page[0].id != first_page[0].id);
    assert!(second_page[0].id != first_page[1].id);
}
