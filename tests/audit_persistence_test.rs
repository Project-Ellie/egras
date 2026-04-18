use egras::audit::model::{AuditCategory, AuditEvent, Outcome};
use egras::audit::persistence::{AuditQueryFilter, AuditRepository, AuditRepositoryPg};
use egras::testing::TestPool;
use uuid::Uuid;

#[tokio::test]
async fn insert_then_list_roundtrip() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    let org = Uuid::now_v7();
    let actor = Uuid::now_v7();
    let e = AuditEvent::organisation_created(actor, org, org, "acme");

    repo.insert(&e).await.unwrap();

    let page = repo
        .list_events(&AuditQueryFilter { limit: 10, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event_type, "organisation.created");
    assert_eq!(page.items[0].outcome, Outcome::Success);
    assert_eq!(page.items[0].category, AuditCategory::TenantsStateChange);
}

#[tokio::test]
async fn filter_by_event_type() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    let actor = Uuid::now_v7();
    let org = Uuid::now_v7();
    repo.insert(&AuditEvent::login_success(actor, org)).await.unwrap();
    repo.insert(&AuditEvent::login_failed("invalid_credentials", "bob")).await.unwrap();

    let page = repo.list_events(&AuditQueryFilter {
        event_type: Some("login.failed".into()),
        limit: 10,
        ..Default::default()
    }).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event_type, "login.failed");
}

#[tokio::test]
async fn cursor_pagination_terminates() {
    let pool = TestPool::fresh().await.pool;
    let repo = AuditRepositoryPg::new(pool.clone());

    for i in 0..5 {
        let mut e = AuditEvent::login_failed("invalid_credentials", &format!("user{i}"));
        // Force distinct occurred_at so ordering is deterministic
        e.occurred_at = chrono::Utc::now() - chrono::Duration::seconds(i);
        repo.insert(&e).await.unwrap();
    }

    let first = repo.list_events(&AuditQueryFilter { limit: 2, ..Default::default() }).await.unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(first.next_cursor.is_some());

    let second = repo.list_events(&AuditQueryFilter {
        limit: 2,
        cursor: first.next_cursor.clone(),
        ..Default::default()
    }).await.unwrap();
    assert_eq!(second.items.len(), 2);

    let third = repo.list_events(&AuditQueryFilter {
        limit: 2,
        cursor: second.next_cursor.clone(),
        ..Default::default()
    }).await.unwrap();
    assert_eq!(third.items.len(), 1);
    assert!(third.next_cursor.is_none());
}
