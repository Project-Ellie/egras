use std::sync::Arc;
use std::time::Duration;

use egras::features::model::FeatureSource;
use egras::features::persistence::{FeaturePgRepository, FeatureRepository};
use egras::features::service::list_definitions::list_definitions;
use egras::features::service::list_org_features::list_org_features;
use egras::features::{FeatureEvaluator, PgFeatureEvaluator};
use egras::testing::TestPool;
use serde_json::json;

use crate::common::seed::{seed_org, seed_user};

const SEEDED_SLUG: &str = "auth.api_key_headers";

fn make_evaluator(repo: Arc<dyn FeatureRepository>) -> PgFeatureEvaluator {
    PgFeatureEvaluator::with_ttl(repo, Duration::from_secs(60))
}

// ---------------------------------------------------------------------------
// 1. list_definitions returns the seeded catalog entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_definitions_includes_seeded_slug() {
    let pool = TestPool::fresh().await.pool;
    let repo = Arc::new(FeaturePgRepository::new(pool));

    let defs = list_definitions(repo.as_ref()).await.unwrap();

    let found = defs.iter().find(|d| d.slug == SEEDED_SLUG);
    assert!(
        found.is_some(),
        "seeded slug '{SEEDED_SLUG}' must be in definitions"
    );

    let def = found.unwrap();
    assert!(
        !def.self_service,
        "auth.api_key_headers should not be self_service"
    );
}

// ---------------------------------------------------------------------------
// 2. list_org_features — mix of overridden + default sources
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_org_features_returns_correct_sources() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "list-test-org", "retail").await;
    let user = seed_user(&pool, "list-test-user").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));
    let evaluator = make_evaluator(repo.clone());

    // Before any override, the single seeded definition should be reported as Default.
    let features_before = list_org_features(repo.as_ref(), &evaluator, org)
        .await
        .unwrap();
    assert!(
        !features_before.is_empty(),
        "should have at least the seeded feature"
    );
    let feat = features_before
        .iter()
        .find(|f| f.slug == SEEDED_SLUG)
        .expect("seeded slug must be present");
    assert_eq!(
        feat.source,
        FeatureSource::Default,
        "no override set yet — source should be Default"
    );

    // Upsert an override.
    let override_val = json!(["x-custom-header"]);
    repo.upsert_override(org, SEEDED_SLUG, override_val.clone(), user)
        .await
        .unwrap();
    // Invalidate so the evaluator sees the new value.
    evaluator.invalidate(org, SEEDED_SLUG).await;

    let features_after = list_org_features(repo.as_ref(), &evaluator, org)
        .await
        .unwrap();
    let feat_after = features_after
        .iter()
        .find(|f| f.slug == SEEDED_SLUG)
        .expect("seeded slug must be present after override");
    assert_eq!(
        feat_after.source,
        FeatureSource::Override,
        "override set — source should be Override"
    );
    assert_eq!(
        feat_after.value, override_val,
        "value should match override"
    );
}
