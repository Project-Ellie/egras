#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use egras::audit::persistence::{AuditRepository, AuditRepositoryPg};
use egras::audit::service::ListAuditEventsImpl;
use egras::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};
use egras::testing::{BlockingAuditRecorder, MockAppStateBuilder, TestPool};

use common::seed::{seed_org, seed_user};

#[tokio::test]
async fn create_organisation_happy_path_returns_summary_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let home = seed_org(&pool, "alice-home", "retail").await;

    let repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(pool.clone()));
    let recorder = Arc::new(BlockingAuditRecorder::new(repo.clone()));
    let state = MockAppStateBuilder::new(pool.clone())
        .audit_recorder(recorder.clone())
        .list_audit_events(Arc::new(ListAuditEventsImpl::new(repo)))
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    let out = create_organisation(
        &state,
        creator,
        home,
        CreateOrganisationInput {
            name: "acme".into(),
            business: "retail".into(),
            seed_creator_as_owner: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(out.name, "acme");
    assert_eq!(out.business, "retail");
    assert_eq!(out.role_codes, vec!["org_owner"]);

    let captured = recorder.captured.lock().await.clone();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].event_type, "organisation.created");
}

#[tokio::test]
async fn create_organisation_duplicate_name_is_conflict() {
    let pool = TestPool::fresh().await.pool;
    let creator = seed_user(&pool, "alice").await;
    let home = seed_org(&pool, "alice-home", "retail").await;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .build();

    create_organisation(
        &state,
        creator,
        home,
        CreateOrganisationInput {
            name: "acme".into(),
            business: "retail".into(),
            seed_creator_as_owner: false,
        },
    )
    .await
    .unwrap();

    let err = create_organisation(
        &state,
        creator,
        home,
        CreateOrganisationInput {
            name: "acme".into(),
            business: "media".into(),
            seed_creator_as_owner: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CreateOrganisationError::DuplicateName));
}
