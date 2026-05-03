use egras::security::service::create_service_account::{
    create_service_account, CreateServiceAccountError, CreateServiceAccountInput,
};
use egras::security::service::delete_service_account::{
    delete_service_account, DeleteServiceAccountError, DeleteServiceAccountInput,
};
use egras::security::service::list_service_accounts::{
    list_service_accounts, ListServiceAccountsInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

use crate::common::seed::{seed_org, seed_user};

fn state(pool: sqlx::PgPool) -> egras::app_state::AppState {
    MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .with_pg_service_account_repos()
        .build()
}

#[tokio::test]
async fn create_happy_path_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool.clone());

    let sa = create_service_account(
        &st,
        CreateServiceAccountInput {
            organisation_id: org,
            name: "billing-bot".into(),
            description: Some("desc".into()),
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    assert_eq!(sa.name, "billing-bot");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events \
         WHERE event_type = 'service_account.created' AND target_id = $1",
    )
    .bind(sa.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn create_duplicate_name_in_same_org_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);

    create_service_account(
        &st,
        CreateServiceAccountInput {
            organisation_id: org,
            name: "bot".into(),
            description: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    let err = create_service_account(
        &st,
        CreateServiceAccountInput {
            organisation_id: org,
            name: "bot".into(),
            description: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CreateServiceAccountError::DuplicateName));
}

#[tokio::test]
async fn list_paginates_within_org() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);

    for i in 0..3 {
        create_service_account(
            &st,
            CreateServiceAccountInput {
                organisation_id: org,
                name: format!("bot-{i}"),
                description: None,
                actor_user_id: actor,
                actor_org_id: org,
            },
        )
        .await
        .unwrap();
    }

    let page1 = list_service_accounts(
        &st,
        ListServiceAccountsInput {
            organisation_id: org,
            limit: 2,
            after: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.next_cursor.is_some());

    let page2 = list_service_accounts(
        &st,
        ListServiceAccountsInput {
            organisation_id: org,
            limit: 2,
            after: page1.next_cursor,
        },
    )
    .await
    .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(page2.next_cursor.is_none());
}

#[tokio::test]
async fn delete_happy_path_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool.clone());

    let sa = create_service_account(
        &st,
        CreateServiceAccountInput {
            organisation_id: org,
            name: "bot".into(),
            description: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    delete_service_account(
        &st,
        DeleteServiceAccountInput {
            organisation_id: org,
            sa_user_id: sa.user_id,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'service_account.deleted'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn delete_cross_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let st = state(pool);

    let sa = create_service_account(
        &st,
        CreateServiceAccountInput {
            organisation_id: org_a,
            name: "bot".into(),
            description: None,
            actor_user_id: actor,
            actor_org_id: org_a,
        },
    )
    .await
    .unwrap();

    let err = delete_service_account(
        &st,
        DeleteServiceAccountInput {
            organisation_id: org_b,
            sa_user_id: sa.user_id,
            actor_user_id: actor,
            actor_org_id: org_b,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DeleteServiceAccountError::NotFound));
}
