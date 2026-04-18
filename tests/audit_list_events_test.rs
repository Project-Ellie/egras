use std::sync::Arc;

use async_trait::async_trait;
use egras::audit::model::AuditEvent;
use egras::audit::persistence::{AuditQueryFilter, AuditQueryPage, AuditRepository};
use egras::audit::service::{ListAuditEvents, ListAuditEventsImpl, ListAuditEventsRequest};
use egras::auth::permissions::PermissionSet;
use uuid::Uuid;

struct StubRepo {
    captured: std::sync::Mutex<Option<AuditQueryFilter>>,
}

#[async_trait]
impl AuditRepository for StubRepo {
    async fn insert(&self, _e: &AuditEvent) -> anyhow::Result<()> { Ok(()) }
    async fn list_events(&self, f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        *self.captured.lock().unwrap() = Some(f.clone());
        Ok(AuditQueryPage { items: vec![], next_cursor: None })
    }
}

#[tokio::test]
async fn read_all_can_query_any_org() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_all".into()]);
    let target_org = Uuid::now_v7();

    svc.execute(
        ListAuditEventsRequest {
            organisation_id: Some(target_org),
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        Uuid::now_v7(),
        &perms,
    ).await.unwrap();

    let f = repo.captured.lock().unwrap().clone().unwrap();
    assert_eq!(f.organisation_id, Some(target_org));
}

#[tokio::test]
async fn read_own_org_with_null_resolves_to_caller_org() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_own_org".into()]);
    let caller_org = Uuid::now_v7();

    svc.execute(
        ListAuditEventsRequest {
            organisation_id: None,
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        caller_org,
        &perms,
    ).await.unwrap();

    let f = repo.captured.lock().unwrap().clone().unwrap();
    assert_eq!(f.organisation_id, Some(caller_org));
}

#[tokio::test]
async fn read_own_org_with_foreign_org_returns_not_found() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["audit.read_own_org".into()]);
    let caller_org = Uuid::now_v7();
    let foreign = Uuid::now_v7();

    let err = svc.execute(
        ListAuditEventsRequest {
            organisation_id: Some(foreign),
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        caller_org,
        &perms,
    ).await.unwrap_err();

    assert!(matches!(err, egras::errors::AppError::NotFound { .. }));
}

#[tokio::test]
async fn no_audit_permission_is_denied() {
    let repo = Arc::new(StubRepo { captured: Default::default() });
    let svc = ListAuditEventsImpl::new(repo.clone());
    let perms = PermissionSet::from_codes(vec!["tenants.read".into()]);

    let err = svc.execute(
        ListAuditEventsRequest {
            organisation_id: None,
            actor_user_id: None, event_type: None, category: None,
            outcome: None, from: None, to: None, cursor: None, limit: None,
        },
        Uuid::now_v7(),
        &perms,
    ).await.unwrap_err();

    assert!(matches!(err, egras::errors::AppError::PermissionDenied { .. }));
}
