use std::sync::Arc;
use std::time::Duration;

use egras::features::persistence::{FeaturePgRepository, FeatureRepository};
use egras::features::{EvaluateError, FeatureEvaluator, PgFeatureEvaluator};
use egras::testing::TestPool;
use serde_json::json;

use crate::common::seed::{seed_org, seed_user};

const SEEDED_SLUG: &str = "auth.api_key_headers";
const SEEDED_DEFAULT: &[&str] = &["x-api-key", "authorization-bearer"];
const UNKNOWN_SLUG: &str = "no.such.flag";

fn make_evaluator(repo: Arc<dyn FeatureRepository>) -> PgFeatureEvaluator {
    // Long TTL — so cache doesn't expire during the test.
    PgFeatureEvaluator::with_ttl(repo, Duration::from_secs(60))
}

// ---------------------------------------------------------------------------
// 1. Known slug, no override → returns catalog default value
// ---------------------------------------------------------------------------

#[tokio::test]
async fn known_slug_no_override_returns_default() {
    let pool = TestPool::fresh().await.pool;
    let repo = Arc::new(FeaturePgRepository::new(pool));
    let evaluator = make_evaluator(repo);

    let value = evaluator
        .evaluate(uuid::Uuid::now_v7(), SEEDED_SLUG)
        .await
        .unwrap();

    let expected = json!(SEEDED_DEFAULT);
    assert_eq!(value, expected, "should return the seeded catalog default");
}

// ---------------------------------------------------------------------------
// 2. Known slug, with override → returns override value
// ---------------------------------------------------------------------------

#[tokio::test]
async fn known_slug_with_override_returns_override() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "eval-test-org-1", "retail").await;
    let user = seed_user(&pool, "eval-test-user-1").await;

    let repo = Arc::new(FeaturePgRepository::new(pool));
    let override_value = json!(["x-custom-header"]);

    repo.upsert_override(org, SEEDED_SLUG, override_value.clone(), user)
        .await
        .unwrap();

    let evaluator = make_evaluator(repo);
    let value = evaluator.evaluate(org, SEEDED_SLUG).await.unwrap();

    assert_eq!(
        value, override_value,
        "should return the org override value"
    );
}

// ---------------------------------------------------------------------------
// 3. Unknown slug → EvaluateError::UnknownSlug
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_slug_returns_unknown_slug_error() {
    let pool = TestPool::fresh().await.pool;
    let repo = Arc::new(FeaturePgRepository::new(pool));
    let evaluator = make_evaluator(repo);

    let err = evaluator
        .evaluate(uuid::Uuid::now_v7(), UNKNOWN_SLUG)
        .await
        .unwrap_err();

    assert!(
        matches!(err, EvaluateError::UnknownSlug),
        "expected UnknownSlug, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 4. Cache demonstrates staleness within TTL
//    Mutate DB row after first evaluate, confirm stale cached value returned.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cached_value_is_served_within_ttl() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "eval-test-org-2", "retail").await;
    let user = seed_user(&pool, "eval-test-user-2").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));

    let first_override = json!(["x-first"]);
    repo.upsert_override(org, SEEDED_SLUG, first_override.clone(), user)
        .await
        .unwrap();

    let evaluator = make_evaluator(repo.clone());

    // First evaluate — caches `first_override`.
    let v1 = evaluator.evaluate(org, SEEDED_SLUG).await.unwrap();
    assert_eq!(
        v1, first_override,
        "sanity: first evaluate returns override"
    );

    // Mutate DB row directly (simulating another process changing the override).
    let second_override = json!(["x-second"]);
    repo.upsert_override(org, SEEDED_SLUG, second_override.clone(), user)
        .await
        .unwrap();

    // Evaluate again BEFORE TTL expires — must return the stale cached value.
    let v2 = evaluator.evaluate(org, SEEDED_SLUG).await.unwrap();
    assert_eq!(
        v2, first_override,
        "within TTL, cached stale value must be returned (not the updated DB value)"
    );
}

// ---------------------------------------------------------------------------
// 5. invalidate(org, slug) clears that cache entry → fresh value on next eval
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalidate_clears_single_entry() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "eval-test-org-3", "retail").await;
    let user = seed_user(&pool, "eval-test-user-3").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));

    let first_override = json!(["x-first"]);
    repo.upsert_override(org, SEEDED_SLUG, first_override.clone(), user)
        .await
        .unwrap();

    let evaluator = make_evaluator(repo.clone());

    // Prime the cache.
    let v1 = evaluator.evaluate(org, SEEDED_SLUG).await.unwrap();
    assert_eq!(v1, first_override);

    // Mutate DB.
    let second_override = json!(["x-second"]);
    repo.upsert_override(org, SEEDED_SLUG, second_override.clone(), user)
        .await
        .unwrap();

    // Invalidate the single entry.
    evaluator.invalidate(org, SEEDED_SLUG).await;

    // Next evaluate should hit the DB and return the fresh value.
    let v2 = evaluator.evaluate(org, SEEDED_SLUG).await.unwrap();
    assert_eq!(
        v2, second_override,
        "after invalidate, fresh value must be fetched from DB"
    );
}

// ---------------------------------------------------------------------------
// 6. invalidate_all() clears all cache entries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalidate_all_clears_all_entries() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "eval-test-org-4a", "retail").await;
    let org2 = seed_org(&pool, "eval-test-org-4b", "media").await;
    let user = seed_user(&pool, "eval-test-user-4").await;

    let repo = Arc::new(FeaturePgRepository::new(pool.clone()));

    let first_val = json!(["x-first"]);
    repo.upsert_override(org1, SEEDED_SLUG, first_val.clone(), user)
        .await
        .unwrap();
    repo.upsert_override(org2, SEEDED_SLUG, first_val.clone(), user)
        .await
        .unwrap();

    let evaluator = make_evaluator(repo.clone());

    // Prime cache for both orgs.
    let v1a = evaluator.evaluate(org1, SEEDED_SLUG).await.unwrap();
    let v1b = evaluator.evaluate(org2, SEEDED_SLUG).await.unwrap();
    assert_eq!(v1a, first_val);
    assert_eq!(v1b, first_val);

    // Mutate both DB rows.
    let second_val = json!(["x-second"]);
    repo.upsert_override(org1, SEEDED_SLUG, second_val.clone(), user)
        .await
        .unwrap();
    repo.upsert_override(org2, SEEDED_SLUG, second_val.clone(), user)
        .await
        .unwrap();

    // invalidate_all — clears all entries.
    evaluator.invalidate_all().await;

    // Both orgs should now return fresh values from DB.
    let v2a = evaluator.evaluate(org1, SEEDED_SLUG).await.unwrap();
    let v2b = evaluator.evaluate(org2, SEEDED_SLUG).await.unwrap();
    assert_eq!(v2a, second_val, "org1: fresh value after invalidate_all");
    assert_eq!(v2b, second_val, "org2: fresh value after invalidate_all");
}
