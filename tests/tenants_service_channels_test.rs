#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use egras::audit::persistence::{AuditRepository, AuditRepositoryPg};
use egras::audit::service::ListAuditEventsImpl;
use egras::tenants::model::ChannelType;
use egras::tenants::service::create_inbound_channel::{
    create_inbound_channel, CreateChannelError, CreateChannelInput,
};
use egras::tenants::service::delete_inbound_channel::delete_inbound_channel;
use egras::tenants::service::get_inbound_channel::{get_inbound_channel, GetChannelError};
use egras::tenants::service::list_inbound_channels::{list_inbound_channels, ListChannelsInput};
use egras::tenants::service::update_inbound_channel::{
    update_inbound_channel, UpdateChannelError, UpdateChannelInput,
};
use egras::testing::{BlockingAuditRecorder, MockAppStateBuilder, TestPool};

use common::seed::{seed_org, seed_user};

fn build_state_with_recorder(
    pool: sqlx::PgPool,
) -> (egras::app_state::AppState, Arc<BlockingAuditRecorder>) {
    let audit_repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(audit_repo.clone()));
    let state = MockAppStateBuilder::new(pool)
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(ListAuditEventsImpl::new(audit_repo)))
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build();
    (state, recorder)
}

fn plain_state(pool: sqlx::PgPool) -> egras::app_state::AppState {
    MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .with_pg_channels_repo()
        .build()
}

#[tokio::test]
async fn create_happy_path_returns_channel_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let (state, recorder) = build_state_with_recorder(pool);

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "VAST Feed".into(),
            description: Some("main feed".into()),
            channel_type: ChannelType::Vast,
            is_active: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(ch.name, "VAST Feed");
    assert_eq!(ch.api_key.len(), 64);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.created");
}

#[tokio::test]
async fn create_duplicate_name_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice2").await;
    let org = seed_org(&pool, "acme2", "retail").await;
    let state = plain_state(pool);

    let input = CreateChannelInput {
        organisation_id: org,
        name: "feed".into(),
        description: None,
        channel_type: ChannelType::Rest,
        is_active: true,
    };
    create_inbound_channel(&state, actor, org, input.clone()).await.unwrap();
    let err = create_inbound_channel(&state, actor, org, input).await.unwrap_err();
    assert!(matches!(err, CreateChannelError::DuplicateName));
}

#[tokio::test]
async fn get_wrong_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice3").await;
    let org1 = seed_org(&pool, "org-k", "retail").await;
    let org2 = seed_org(&pool, "org-l", "retail").await;
    let state = plain_state(pool);

    let ch = create_inbound_channel(
        &state,
        actor,
        org1,
        CreateChannelInput {
            organisation_id: org1,
            name: "feed".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let err = get_inbound_channel(&state, org2, ch.id).await.unwrap_err();
    assert!(matches!(err, GetChannelError::NotFound));
}

#[tokio::test]
async fn list_returns_all_channels_for_org() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice4").await;
    let org = seed_org(&pool, "org-m", "retail").await;
    let state = plain_state(pool);

    for i in 0..3 {
        create_inbound_channel(
            &state,
            actor,
            org,
            CreateChannelInput {
                organisation_id: org,
                name: format!("feed-{i}"),
                description: None,
                channel_type: ChannelType::Rest,
                is_active: true,
            },
        )
        .await
        .unwrap();
    }

    let out = list_inbound_channels(
        &state,
        ListChannelsInput {
            organisation_id: org,
            after: None,
            limit: 50,
        },
    )
    .await
    .unwrap();
    assert_eq!(out.items.len(), 3);
    assert!(out.next_cursor.is_none());
}

#[tokio::test]
async fn list_pagination_produces_next_cursor_when_more_items_exist() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice5").await;
    let org = seed_org(&pool, "org-n", "retail").await;
    let state = plain_state(pool);

    for i in 0..3 {
        create_inbound_channel(
            &state,
            actor,
            org,
            CreateChannelInput {
                organisation_id: org,
                name: format!("feed-pag-{i}"),
                description: None,
                channel_type: ChannelType::Rest,
                is_active: true,
            },
        )
        .await
        .unwrap();
    }

    let page1 = list_inbound_channels(
        &state,
        ListChannelsInput {
            organisation_id: org,
            after: None,
            limit: 2,
        },
    )
    .await
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.next_cursor.is_some());

    let page2 = list_inbound_channels(
        &state,
        ListChannelsInput {
            organisation_id: org,
            after: page1.next_cursor,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(page2.next_cursor.is_none());
}

#[tokio::test]
async fn update_changes_fields_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice6").await;
    let org = seed_org(&pool, "org-o", "retail").await;
    let (state, recorder) = build_state_with_recorder(pool);

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "old".into(),
            description: None,
            channel_type: ChannelType::Vast,
            is_active: true,
        },
    )
    .await
    .unwrap();

    recorder.captured.lock().await.clear();

    let updated = update_inbound_channel(
        &state,
        actor,
        org,
        UpdateChannelInput {
            organisation_id: org,
            channel_id: ch.id,
            name: "new".into(),
            description: Some("desc".into()),
            channel_type: ChannelType::Sensor,
            is_active: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.name, "new");
    assert_eq!(updated.api_key, ch.api_key);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.updated");
}

#[tokio::test]
async fn delete_emits_audit_and_get_returns_not_found_after() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice7").await;
    let org = seed_org(&pool, "org-p", "retail").await;
    let (state, recorder) = build_state_with_recorder(pool);

    let ch = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "to-delete".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    recorder.captured.lock().await.clear();

    delete_inbound_channel(&state, actor, org, org, ch.id).await.unwrap();

    let err = get_inbound_channel(&state, org, ch.id).await.unwrap_err();
    assert!(matches!(err, GetChannelError::NotFound));

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "channel.deleted");
}

#[tokio::test]
async fn update_name_collision_returns_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice8").await;
    let org = seed_org(&pool, "org-q", "retail").await;
    let state = plain_state(pool);

    create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "taken".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let ch2 = create_inbound_channel(
        &state,
        actor,
        org,
        CreateChannelInput {
            organisation_id: org,
            name: "other".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap();

    let err = update_inbound_channel(
        &state,
        actor,
        org,
        UpdateChannelInput {
            organisation_id: org,
            channel_id: ch2.id,
            name: "taken".into(),
            description: None,
            channel_type: ChannelType::Rest,
            is_active: true,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, UpdateChannelError::DuplicateName));
}
