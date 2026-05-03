use egras::outbox::persistence::{OutboxRepository, OutboxRepositoryPg};
use egras::outbox::AppendRequest;
use egras::testing::TestPool;
use serde_json::json;
use uuid::Uuid;

fn req(event_type: &str) -> AppendRequest {
    AppendRequest::new(event_type, json!({"k": event_type}))
}

#[tokio::test]
async fn append_in_tx_then_commit_row_visible() {
    let pool = TestPool::fresh().await.pool;
    let repo = OutboxRepositoryPg::new(pool.clone());

    let mut tx = pool.begin().await.unwrap();
    let id = repo
        .append_in_tx(&mut tx, req("user.created"))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let evt = repo
        .find(id)
        .await
        .unwrap()
        .expect("row visible after commit");
    assert_eq!(evt.event_type, "user.created");
    assert_eq!(evt.payload, json!({"k": "user.created"}));
    assert!(evt.relayed_at.is_none());
    assert_eq!(evt.relay_attempts, 0);
}

#[tokio::test]
async fn append_in_tx_then_rollback_row_absent() {
    let pool = TestPool::fresh().await.pool;
    let repo = OutboxRepositoryPg::new(pool.clone());

    let mut tx = pool.begin().await.unwrap();
    let id = repo
        .append_in_tx(&mut tx, req("rolled.back"))
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert!(repo.find(id).await.unwrap().is_none());
}

#[tokio::test]
async fn claim_unrelayed_returns_events_in_created_at_ascending() {
    let pool = TestPool::fresh().await.pool;
    let repo = OutboxRepositoryPg::new(pool.clone());

    let id_a = repo.append_standalone(req("a")).await.unwrap();
    let id_b = repo.append_standalone(req("b")).await.unwrap();
    let id_c = repo.append_standalone(req("c")).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let claimed = repo.claim_unrelayed_in_tx(&mut tx, 10).await.unwrap();
    tx.rollback().await.unwrap(); // release locks; we only inspect order

    let ids: Vec<Uuid> = claimed.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![id_a, id_b, id_c]);
}

#[tokio::test]
async fn concurrent_claims_partition_via_skip_locked() {
    let pool = TestPool::fresh().await.pool;
    let repo = OutboxRepositoryPg::new(pool.clone());

    for _ in 0..6 {
        repo.append_standalone(req("evt")).await.unwrap();
    }

    let mut tx_a = pool.begin().await.unwrap();
    let claim_a = repo.claim_unrelayed_in_tx(&mut tx_a, 3).await.unwrap();
    assert_eq!(claim_a.len(), 3, "first claimer takes its full batch");

    // Concurrent transaction must see the remaining 3 (SKIP LOCKED bypasses
    // the rows held by tx_a).
    let mut tx_b = pool.begin().await.unwrap();
    let claim_b = repo.claim_unrelayed_in_tx(&mut tx_b, 10).await.unwrap();
    assert_eq!(claim_b.len(), 3, "second claimer skips locked rows");

    let ids_a: std::collections::HashSet<Uuid> = claim_a.iter().map(|e| e.id).collect();
    let ids_b: std::collections::HashSet<Uuid> = claim_b.iter().map(|e| e.id).collect();
    assert!(
        ids_a.is_disjoint(&ids_b),
        "the two batches must be disjoint"
    );

    tx_a.rollback().await.unwrap();
    tx_b.rollback().await.unwrap();
}

#[tokio::test]
async fn mark_relayed_in_tx_sets_relayed_at_for_all_ids() {
    let pool = TestPool::fresh().await.pool;
    let repo = OutboxRepositoryPg::new(pool.clone());

    let mut ids = Vec::new();
    for n in 0..4 {
        let id = repo
            .append_standalone(req(&format!("e-{n}")))
            .await
            .unwrap();
        ids.push(id);
    }

    let mut tx = pool.begin().await.unwrap();
    repo.mark_relayed_in_tx(&mut tx, &ids).await.unwrap();
    tx.commit().await.unwrap();

    for id in &ids {
        let evt = repo.find(*id).await.unwrap().expect("row exists");
        assert!(evt.relayed_at.is_some(), "id {id} should be marked relayed");
    }

    // Empty input is a no-op (must not error or touch other rows).
    let extra = repo.append_standalone(req("untouched")).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    repo.mark_relayed_in_tx(&mut tx, &[]).await.unwrap();
    tx.commit().await.unwrap();
    let evt = repo.find(extra).await.unwrap().unwrap();
    assert!(evt.relayed_at.is_none());
}
