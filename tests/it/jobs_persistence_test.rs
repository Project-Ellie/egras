use std::time::Duration;

use chrono::Utc;
use egras::jobs::model::{EnqueueRequest, JobState};
use egras::jobs::persistence::{JobsRepository, JobsRepositoryPg};
use egras::testing::TestPool;

const KIND: &str = "test.kind";

fn req() -> EnqueueRequest {
    EnqueueRequest::now(KIND, serde_json::json!({"hello": "world"})).with_max_attempts(3)
}

#[tokio::test]
async fn enqueue_then_find() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();
    let job = repo.find(id).await.unwrap().expect("job exists");
    assert_eq!(job.kind, KIND);
    assert_eq!(job.state, JobState::Pending);
    assert_eq!(job.attempts, 0);
    assert_eq!(job.max_attempts, 3);
    assert_eq!(job.payload, serde_json::json!({"hello": "world"}));
    assert!(job.locked_until.is_none());
}

#[tokio::test]
async fn claim_due_marks_running_and_locks() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();

    let claimed = repo
        .claim_due("worker-a", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, id);
    assert_eq!(claimed[0].state, JobState::Running);
    assert_eq!(claimed[0].locked_by.as_deref(), Some("worker-a"));
    assert!(claimed[0].locked_until.is_some());

    // Second claim sees nothing — already locked.
    let claimed2 = repo
        .claim_due("worker-b", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    assert!(claimed2.is_empty());
}

#[tokio::test]
async fn scheduled_job_not_claimed_before_run_at() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let future = Utc::now() + chrono::Duration::seconds(60);
    let id = repo.enqueue(req().with_run_at(future)).await.unwrap();

    let claimed = repo
        .claim_due("w", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    assert!(
        claimed.is_empty(),
        "future-scheduled job must not be claimed"
    );

    // sanity: still pending
    let job = repo.find(id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Pending);
}

#[tokio::test]
async fn lock_expiry_allows_reclaim() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();

    // First claim with a 1-second visibility.
    let claimed = repo
        .claim_due("worker-a", &[KIND.to_string()], Duration::from_secs(1), 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);

    // Wait past the lock.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Another worker re-claims the same job.
    let reclaimed = repo
        .claim_due("worker-b", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].id, id);
    assert_eq!(reclaimed[0].locked_by.as_deref(), Some("worker-b"));
}

#[tokio::test]
async fn mark_done_terminal() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();
    let _ = repo
        .claim_due("w", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    repo.mark_done(id).await.unwrap();

    let job = repo.find(id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Done);
    assert!(job.locked_until.is_none());
    assert!(job.locked_by.is_none());
}

#[tokio::test]
async fn mark_failed_retry_increments_attempts_and_reschedules() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();
    let _ = repo
        .claim_due("w", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();

    let next = Utc::now() + chrono::Duration::seconds(120);
    repo.mark_failed_retry(id, "boom", next).await.unwrap();

    let job = repo.find(id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Pending);
    assert_eq!(job.attempts, 1);
    assert_eq!(job.last_error.as_deref(), Some("boom"));
    assert!(job.locked_until.is_none());
    // run_at moved into the future
    assert!(job.run_at > Utc::now() + chrono::Duration::seconds(60));
}

#[tokio::test]
async fn mark_dead_terminal() {
    let pool = TestPool::fresh().await.pool;
    let repo = JobsRepositoryPg::new(pool);

    let id = repo.enqueue(req()).await.unwrap();
    let _ = repo
        .claim_due("w", &[KIND.to_string()], Duration::from_secs(30), 10)
        .await
        .unwrap();
    repo.mark_dead(id, "fatal").await.unwrap();

    let job = repo.find(id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Dead);
    assert_eq!(job.last_error.as_deref(), Some("fatal"));
}
