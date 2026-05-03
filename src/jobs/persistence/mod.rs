mod jobs_repository_pg;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;

use crate::jobs::model::{EnqueueRequest, Job};

pub use jobs_repository_pg::JobsRepositoryPg;

#[async_trait]
pub trait JobsRepository: Send + Sync + 'static {
    async fn enqueue(&self, req: EnqueueRequest) -> anyhow::Result<Uuid>;

    /// Atomically claim up to `limit` due jobs of any of the given `kinds`,
    /// transitioning them to `running` with `locked_until = now + visibility`
    /// and `locked_by = worker_id`.
    ///
    /// Eligible: `state = 'pending' AND run_at <= now`
    ///        OR `state = 'running' AND locked_until <= now` (lock expired).
    async fn claim_due(
        &self,
        worker_id: &str,
        kinds: &[String],
        visibility: Duration,
        limit: u32,
    ) -> anyhow::Result<Vec<Job>>;

    async fn mark_done(&self, id: Uuid) -> anyhow::Result<()>;

    /// Increment attempts, set state back to `pending`, schedule next run.
    async fn mark_failed_retry(
        &self,
        id: Uuid,
        error: &str,
        next_run_at: DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// Mark as `dead` (no further retries).
    async fn mark_dead(&self, id: Uuid, error: &str) -> anyhow::Result<()>;

    async fn find(&self, id: Uuid) -> anyhow::Result<Option<Job>>;
}
