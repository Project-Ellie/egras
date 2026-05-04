use std::sync::Arc;
use std::time::Duration;

use egras::audit::persistence::AuditRepositoryPg;
use egras::audit::service::AuditRecorder;
use egras::features::persistence::{FeaturePgRepository, FeatureRepository};
use egras::features::service::clear_org_feature::{
    clear_org_feature, ClearOrgFeatureError, ClearOrgFeatureInput,
};
use egras::features::service::set_org_feature::{
    set_org_feature, SetOrgFeatureError, SetOrgFeatureInput,
};
use egras::features::{FeatureEvaluator, PgFeatureEvaluator};
use egras::testing::{BlockingAuditRecorder, TestPool};
use serde_json::json;

use crate::common::fixtures::OPERATOR_ORG_ID;
use crate::common::seed::{seed_org, seed_user};

const SLUG: &str = "auth.api_key_headers";
const UNKNOWN_SLUG: &str = "no.such.flag";

fn make_blocking_audit(pool: &sqlx::PgPool) -> Arc<BlockingAuditRecorder> {
    let repo = Arc::new(AuditRepositoryPg::new(pool.clone()));
    Arc::new(BlockingAuditRecorder::new(repo))
}

fn make_evaluator(repo: Arc<dyn FeatureRepository>) -> PgFeatureEvaluator {
    PgFeatureEvaluator::with_ttl(repo, Duration::from_secs(60))
}

// ---------------------------------------------------------------------------
// 1. Operator sets a non-self-service flag — happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn operator_set_non_self_service_succeeds_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "op-admin-1").await;
    let org = seed_org(&pool, "tenant-1", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    let new_val = json!(["x-custom-key"]);

    set_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        SetOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            value: new_val.clone(),
            actor_user_id: actor,
            actor_org_id: OPERATOR_ORG_ID,
            actor_is_operator: true,
        },
    )
    .await
    .expect("operator should be allowed to set a non-self-service flag");

    // Verify cache was invalidated — evaluator must return the new value.
    let effective = evaluator.evaluate(org, SLUG).await.unwrap();
    assert_eq!(
        effective, new_val,
        "evaluator should return new value after invalidation"
    );

    // Verify audit event was emitted.
    let captured = audit.captured.lock().await;
    let events: Vec<_> = captured
        .iter()
        .filter(|e| e.event_type == "feature.set")
        .collect();
    assert_eq!(events.len(), 1, "exactly one feature.set audit event");
    let ev = &events[0];
    assert_eq!(ev.payload["slug"], json!(SLUG));
    assert_eq!(ev.payload["new_value"], new_val);
    assert_eq!(ev.payload["self_service"], json!(false));
}

// ---------------------------------------------------------------------------
// 2. Non-operator tries to set a non-self-service flag → NotSelfService
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_operator_set_non_self_service_flag_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "org-admin-1").await;
    let org = seed_org(&pool, "tenant-2", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    // auth.api_key_headers has self_service=false in the seed.
    let err = set_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        SetOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            value: json!(["x-custom"]),
            actor_user_id: actor,
            actor_org_id: org,
            actor_is_operator: false,
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, SetOrgFeatureError::NotSelfService),
        "expected NotSelfService, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. Type mismatch → InvalidValue (checked before self_service guard)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn type_mismatch_returns_invalid_value() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "op-admin-2").await;
    let org = seed_org(&pool, "tenant-3", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    // SLUG is EnumSet type; passing a bool should fail.
    let err = set_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        SetOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            value: json!(true), // wrong type
            actor_user_id: actor,
            actor_org_id: OPERATOR_ORG_ID,
            actor_is_operator: true,
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, SetOrgFeatureError::InvalidValue(_)),
        "expected InvalidValue, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 4. Unknown slug → UnknownSlug
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_slug_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "op-admin-3").await;
    let org = seed_org(&pool, "tenant-4", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    let err = set_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        SetOrgFeatureInput {
            organisation_id: org,
            slug: UNKNOWN_SLUG.into(),
            value: json!(true),
            actor_user_id: actor,
            actor_org_id: OPERATOR_ORG_ID,
            actor_is_operator: true,
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, SetOrgFeatureError::UnknownSlug),
        "expected UnknownSlug, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 5. clear_org_feature happy path — operator clears, audit emitted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn operator_clear_succeeds_and_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "op-admin-4").await;
    let org = seed_org(&pool, "tenant-5", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    // First set an override so there's something to clear.
    set_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        SetOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            value: json!(["x-to-clear"]),
            actor_user_id: actor,
            actor_org_id: OPERATOR_ORG_ID,
            actor_is_operator: true,
        },
    )
    .await
    .unwrap();

    // Now clear it.
    clear_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        ClearOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            actor_user_id: actor,
            actor_org_id: OPERATOR_ORG_ID,
            actor_is_operator: true,
        },
    )
    .await
    .expect("operator clear should succeed");

    // After clearing, evaluator should return the default.
    let effective = evaluator.evaluate(org, SLUG).await.unwrap();
    let expected_default = json!(["x-api-key", "authorization-bearer"]);
    assert_eq!(
        effective, expected_default,
        "should revert to default after clear"
    );

    // Audit: one feature.cleared event.
    let captured = audit.captured.lock().await;
    let clear_events: Vec<_> = captured
        .iter()
        .filter(|e| e.event_type == "feature.cleared")
        .collect();
    assert_eq!(
        clear_events.len(),
        1,
        "exactly one feature.cleared audit event"
    );
    assert_eq!(clear_events[0].payload["slug"], json!(SLUG));
}

// ---------------------------------------------------------------------------
// 6. Non-operator tries to clear a non-self-service flag → NotSelfService
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_operator_clear_non_self_service_flag_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "org-admin-2").await;
    let org = seed_org(&pool, "tenant-6", "retail").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let audit = make_blocking_audit(&pool);
    let evaluator = make_evaluator(repo.clone());

    let err = clear_org_feature(
        repo.as_ref(),
        &evaluator,
        audit.as_ref() as &dyn AuditRecorder,
        ClearOrgFeatureInput {
            organisation_id: org,
            slug: SLUG.into(),
            actor_user_id: actor,
            actor_org_id: org,
            actor_is_operator: false,
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, ClearOrgFeatureError::NotSelfService),
        "expected NotSelfService, got: {err:?}"
    );
}
