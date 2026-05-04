use egras::features::persistence::{FeaturePgRepository, FeatureRepoError, FeatureRepository};
use egras::testing::TestPool;
use serde_json::json;

use crate::common::seed::{seed_org, seed_user};

// The seeded slug from migration 0012.
const SEEDED_SLUG: &str = "auth.api_key_headers";
const UNKNOWN_SLUG: &str = "no.such.flag";

// ---------------------------------------------------------------------------
// list_definitions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_definitions_returns_seeded_catalog() {
    let pool = TestPool::fresh().await.pool;
    let repo = FeaturePgRepository::new(pool);

    let defs = repo.list_definitions().await.unwrap();
    assert!(
        !defs.is_empty(),
        "catalog must contain at least the seeded row"
    );
    assert!(
        defs.iter().any(|d| d.slug == SEEDED_SLUG),
        "seeded slug must appear in catalog"
    );
}

// ---------------------------------------------------------------------------
// get_definition
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_definition_returns_some_for_known_slug() {
    let pool = TestPool::fresh().await.pool;
    let repo = FeaturePgRepository::new(pool);

    let def = repo.get_definition(SEEDED_SLUG).await.unwrap();
    assert!(def.is_some());
    let def = def.unwrap();
    assert_eq!(def.slug, SEEDED_SLUG);
    assert!(!def.self_service);
}

#[tokio::test]
async fn get_definition_returns_none_for_unknown_slug() {
    let pool = TestPool::fresh().await.pool;
    let repo = FeaturePgRepository::new(pool);

    let def = repo.get_definition(UNKNOWN_SLUG).await.unwrap();
    assert!(def.is_none());
}

// ---------------------------------------------------------------------------
// upsert_override — insert then read back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_override_then_read_back() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "feat-test-org-1", "retail").await;
    let user = seed_user(&pool, "feat-test-user-1").await;
    let repo = FeaturePgRepository::new(pool);

    let new_value = json!(["x-api-key"]);

    // First insert: no previous value.
    let prev = repo
        .upsert_override(org, SEEDED_SLUG, new_value.clone(), user)
        .await
        .unwrap();
    assert!(
        prev.is_none(),
        "first insert must return None for old value"
    );

    // Read back via get_override.
    let ov = repo.get_override(org, SEEDED_SLUG).await.unwrap();
    assert!(ov.is_some());
    let ov = ov.unwrap();
    assert_eq!(ov.organisation_id, org);
    assert_eq!(ov.slug, SEEDED_SLUG);
    assert_eq!(ov.value, new_value);
    assert_eq!(ov.updated_by, user);
}

// ---------------------------------------------------------------------------
// upsert_override — returns old value on update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_returns_old_value_on_update() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "feat-test-org-2", "retail").await;
    let user = seed_user(&pool, "feat-test-user-2").await;
    let repo = FeaturePgRepository::new(pool);

    let first_value = json!(["x-api-key"]);
    let second_value = json!(["authorization-bearer"]);

    // Insert.
    repo.upsert_override(org, SEEDED_SLUG, first_value.clone(), user)
        .await
        .unwrap();

    // Update: old value must be returned.
    let prev = repo
        .upsert_override(org, SEEDED_SLUG, second_value.clone(), user)
        .await
        .unwrap();
    assert_eq!(prev, Some(first_value));

    // Confirm new value stored.
    let ov = repo.get_override(org, SEEDED_SLUG).await.unwrap().unwrap();
    assert_eq!(ov.value, second_value);
}

// ---------------------------------------------------------------------------
// delete_override — returns old value
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_returns_old_value() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "feat-test-org-3", "retail").await;
    let user = seed_user(&pool, "feat-test-user-3").await;
    let repo = FeaturePgRepository::new(pool);

    let value = json!(["x-api-key"]);

    repo.upsert_override(org, SEEDED_SLUG, value.clone(), user)
        .await
        .unwrap();

    let prev = repo.delete_override(org, SEEDED_SLUG).await.unwrap();
    assert_eq!(prev, Some(value));

    // Gone.
    let ov = repo.get_override(org, SEEDED_SLUG).await.unwrap();
    assert!(ov.is_none());
}

#[tokio::test]
async fn delete_returns_none_when_no_override_present() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "feat-test-org-4", "retail").await;
    let repo = FeaturePgRepository::new(pool);

    let prev = repo.delete_override(org, SEEDED_SLUG).await.unwrap();
    assert!(prev.is_none());
}

// ---------------------------------------------------------------------------
// list_overrides_for_org
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_overrides_returns_only_org_overrides() {
    let pool = TestPool::fresh().await.pool;
    let org1 = seed_org(&pool, "feat-test-org-5", "retail").await;
    let org2 = seed_org(&pool, "feat-test-org-6", "media").await;
    let user = seed_user(&pool, "feat-test-user-5").await;
    let repo = FeaturePgRepository::new(pool);

    let value = json!(["x-api-key"]);

    repo.upsert_override(org1, SEEDED_SLUG, value.clone(), user)
        .await
        .unwrap();

    // org2 has no overrides.
    let list2 = repo.list_overrides_for_org(org2).await.unwrap();
    assert!(list2.is_empty());

    // org1 has one override.
    let list1 = repo.list_overrides_for_org(org1).await.unwrap();
    assert_eq!(list1.len(), 1);
    assert_eq!(list1[0].slug, SEEDED_SLUG);
    assert_eq!(list1[0].organisation_id, org1);
}

// ---------------------------------------------------------------------------
// Unknown-slug rejection (FK violation → UnknownSlug)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_unknown_slug_returns_unknown_slug_error() {
    let pool = TestPool::fresh().await.pool;
    let org = seed_org(&pool, "feat-test-org-7", "retail").await;
    let user = seed_user(&pool, "feat-test-user-7").await;
    let repo = FeaturePgRepository::new(pool);

    let err = repo
        .upsert_override(org, UNKNOWN_SLUG, json!(true), user)
        .await
        .unwrap_err();
    assert!(
        matches!(err, FeatureRepoError::UnknownSlug),
        "expected UnknownSlug, got: {err}"
    );
}
