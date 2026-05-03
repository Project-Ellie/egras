use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egras::jobs::model::{EnqueueRequest, Job, JobState};
use egras::jobs::persistence::{JobsRepository, JobsRepositoryPg};
use egras::jobs::runner::{JobError, JobHandler, JobRunner, JobRunnerConfig};
use egras::outbox::persistence::{OutboxRepository, OutboxRepositoryPg};
use egras::outbox::{AppendRequest, OutboxRelayer, OutboxRelayerConfig};
use egras::testing::TestPool;
use serde_json::json;
use uuid::Uuid;

fn fast_relayer_cfg() -> OutboxRelayerConfig {
    OutboxRelayerConfig {
        poll_interval: Duration::from_millis(20),
        batch_size: 32,
        job_max_attempts: 3,
    }
}

fn fast_runner_cfg() -> JobRunnerConfig {
    JobRunnerConfig {
        poll_interval: Duration::from_millis(20),
        visibility_timeout: Duration::from_secs(30),
        batch_size: 16,
        backoff_initial: Duration::from_millis(1),
        backoff_factor: 2,
        backoff_max: Duration::from_millis(50),
    }
}

#[tokio::test]
async fn tick_relays_appended_events_to_jobs_and_marks_outbox() {
    let pool = TestPool::fresh().await.pool;

    let outbox_pg = Arc::new(OutboxRepositoryPg::new(pool.clone()));
    let outbox_repo: Arc<dyn OutboxRepository> = outbox_pg.clone();
    let jobs_pg = Arc::new(JobsRepositoryPg::new(pool.clone()));
    let jobs_repo: Arc<dyn JobsRepository> = jobs_pg.clone();

    let evt_id = outbox_pg
        .append_standalone(AppendRequest::new(
            "user.created",
            json!({"user_id": "u-1"}),
        ))
        .await
        .unwrap();

    let relayer = OutboxRelayer::new(
        pool.clone(),
        outbox_repo.clone(),
        jobs_repo.clone(),
        fast_relayer_cfg(),
    );
    let relayed = relayer.tick().await.unwrap();
    assert_eq!(relayed, 1, "exactly one event should be relayed");

    let evt = outbox_pg.find(evt_id).await.unwrap().unwrap();
    assert!(evt.relayed_at.is_some(), "outbox row marked relayed");

    // Exactly one job of the expected kind exists.
    let jobs = sqlx::query_as::<_, (Uuid, String, serde_json::Value)>(
        "SELECT id, kind, payload FROM jobs",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].1, "user.created");
    assert_eq!(jobs[0].2, json!({"user_id": "u-1"}));

    // A second tick is a no-op.
    let again = relayer.tick().await.unwrap();
    assert_eq!(again, 0, "no unrelayed events left");
}

/// Wrap a real jobs repo but force `enqueue_in_tx` to fail; verify the outbox
/// transaction rolls back so the event stays unrelayed and is retried.
struct FailingEnqueueJobs {
    inner: Arc<JobsRepositoryPg>,
    fail_count: AtomicU32,
}

#[async_trait]
impl JobsRepository for FailingEnqueueJobs {
    async fn enqueue(&self, req: EnqueueRequest) -> anyhow::Result<Uuid> {
        self.inner.enqueue(req).await
    }
    async fn enqueue_in_tx(
        &self,
        _tx: &mut sqlx::PgConnection,
        _req: EnqueueRequest,
    ) -> anyhow::Result<Uuid> {
        self.fail_count.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("simulated enqueue failure")
    }
    async fn claim_due(
        &self,
        worker_id: &str,
        kinds: &[String],
        visibility: Duration,
        limit: u32,
    ) -> anyhow::Result<Vec<Job>> {
        self.inner
            .claim_due(worker_id, kinds, visibility, limit)
            .await
    }
    async fn mark_done(&self, id: Uuid) -> anyhow::Result<()> {
        self.inner.mark_done(id).await
    }
    async fn mark_failed_retry(
        &self,
        id: Uuid,
        error: &str,
        next_run_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.inner.mark_failed_retry(id, error, next_run_at).await
    }
    async fn mark_dead(&self, id: Uuid, error: &str) -> anyhow::Result<()> {
        self.inner.mark_dead(id, error).await
    }
    async fn find(&self, id: Uuid) -> anyhow::Result<Option<Job>> {
        self.inner.find(id).await
    }
}

#[tokio::test]
async fn enqueue_failure_keeps_outbox_unrelayed() {
    let pool = TestPool::fresh().await.pool;

    let outbox_pg = Arc::new(OutboxRepositoryPg::new(pool.clone()));
    let outbox_repo: Arc<dyn OutboxRepository> = outbox_pg.clone();
    let failing = Arc::new(FailingEnqueueJobs {
        inner: Arc::new(JobsRepositoryPg::new(pool.clone())),
        fail_count: AtomicU32::new(0),
    });
    let jobs_repo: Arc<dyn JobsRepository> = failing.clone();

    let evt_id = outbox_pg
        .append_standalone(AppendRequest::new("welcome.email", json!({})))
        .await
        .unwrap();

    let relayer = OutboxRelayer::new(pool.clone(), outbox_repo, jobs_repo, fast_relayer_cfg());
    let result = relayer.tick().await;
    assert!(result.is_err(), "tick must surface the enqueue failure");
    assert!(failing.fail_count.load(Ordering::SeqCst) >= 1);

    // Outbox row stays unrelayed; no jobs were inserted (tx rolled back).
    let evt = outbox_pg.find(evt_id).await.unwrap().unwrap();
    assert!(
        evt.relayed_at.is_none(),
        "transaction rollback must leave outbox unrelayed"
    );
    let job_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(job_count, 0, "no jobs should have been committed");
}

struct CapturingHandler {
    seen: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
}

#[async_trait]
impl JobHandler for CapturingHandler {
    fn kind(&self) -> &'static str {
        "order.placed"
    }
    async fn handle(&self, payload: &serde_json::Value) -> Result<(), JobError> {
        self.seen.lock().await.push(payload.clone());
        Ok(())
    }
}

#[tokio::test]
async fn end_to_end_append_relay_handler_runs() {
    let pool = TestPool::fresh().await.pool;

    let outbox_pg = Arc::new(OutboxRepositoryPg::new(pool.clone()));
    let outbox_repo: Arc<dyn OutboxRepository> = outbox_pg.clone();
    let jobs_pg = Arc::new(JobsRepositoryPg::new(pool.clone()));
    let jobs_repo: Arc<dyn JobsRepository> = jobs_pg.clone();

    let seen: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let handler = Arc::new(CapturingHandler { seen: seen.clone() });

    let runner_handle = JobRunner::new(jobs_repo.clone(), fast_runner_cfg())
        .register(handler)
        .spawn();
    let relayer_handle = OutboxRelayer::new(
        pool.clone(),
        outbox_repo,
        jobs_repo.clone(),
        fast_relayer_cfg(),
    )
    .spawn();

    let payload = json!({"order_id": "o-42"});
    outbox_pg
        .append_standalone(AppendRequest::new("order.placed", payload.clone()))
        .await
        .unwrap();

    // Wait up to 3s for the handler to observe the payload.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !seen.lock().await.is_empty() {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("handler did not observe payload within timeout");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(seen.lock().await[0], payload);

    // The corresponding job should now be done.
    let job = sqlx::query_as::<_, (String,)>("SELECT state FROM jobs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(job.0, JobState::Done.as_str());

    relayer_handle.shutdown().await;
    runner_handle.shutdown().await;
}

#[tokio::test]
async fn graceful_shutdown_completes() {
    let pool = TestPool::fresh().await.pool;
    let outbox: Arc<dyn OutboxRepository> = Arc::new(OutboxRepositoryPg::new(pool.clone()));
    let jobs: Arc<dyn JobsRepository> = Arc::new(JobsRepositoryPg::new(pool.clone()));
    let handle = OutboxRelayer::new(pool, outbox, jobs, fast_relayer_cfg()).spawn();
    handle.shutdown().await;
}
