use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::jobs::model::{EnqueueRequest, Job};
use crate::jobs::persistence::JobsRepository;

/// Narrow facade for service code: only `enqueue`. The runner consumes the
/// full [`JobsRepository`] separately.
#[async_trait]
pub trait JobsEnqueuer: Send + Sync + 'static {
    async fn enqueue(&self, req: EnqueueRequest) -> anyhow::Result<Uuid>;
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("retryable: {0}")]
    Retryable(String),
    #[error("permanent: {0}")]
    Permanent(String),
}

#[async_trait]
pub trait JobHandler: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    async fn handle(&self, payload: &serde_json::Value) -> Result<(), JobError>;
}

#[derive(Debug, Clone)]
pub struct JobRunnerConfig {
    pub poll_interval: Duration,
    pub visibility_timeout: Duration,
    pub batch_size: u32,
    pub backoff_initial: Duration,
    pub backoff_factor: u32,
    pub backoff_max: Duration,
}

impl Default for JobRunnerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(500),
            visibility_timeout: Duration::from_secs(60),
            batch_size: 16,
            backoff_initial: Duration::from_secs(5),
            backoff_factor: 4,
            backoff_max: Duration::from_secs(3600),
        }
    }
}

pub struct JobRunner {
    repo: Arc<dyn JobsRepository>,
    handlers: HashMap<&'static str, Arc<dyn JobHandler>>,
    worker_id: String,
    cfg: JobRunnerConfig,
}

pub struct JobRunnerHandle {
    task: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
}

impl JobRunnerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        if let Err(err) = self.task.await {
            tracing::error!(error = %err, "job runner task join error");
        }
    }
}

impl JobRunner {
    pub fn new(repo: Arc<dyn JobsRepository>, cfg: JobRunnerConfig) -> Self {
        let worker_id = format!(
            "{}-{}",
            hostname().unwrap_or_else(|| "unknown".to_string()),
            Uuid::now_v7().simple()
        );
        Self {
            repo,
            handlers: HashMap::new(),
            worker_id,
            cfg,
        }
    }

    pub fn register(mut self, handler: Arc<dyn JobHandler>) -> Self {
        self.handlers.insert(handler.kind(), handler);
        self
    }

    pub fn registered_kinds(&self) -> Vec<String> {
        self.handlers.keys().map(|s| s.to_string()).collect()
    }

    pub fn spawn(self) -> JobRunnerHandle {
        let (tx, rx) = watch::channel(false);
        let task = tokio::spawn(self.run(rx));
        JobRunnerHandle { task, shutdown: tx }
    }

    async fn run(self, mut shutdown: watch::Receiver<bool>) {
        tracing::info!(worker_id = %self.worker_id, kinds = ?self.handlers.keys().collect::<Vec<_>>(), "job runner started");
        let kinds = self.registered_kinds();
        if kinds.is_empty() {
            tracing::warn!("job runner has no registered handlers; idling");
        }
        loop {
            if *shutdown.borrow() {
                break;
            }
            let claimed = match self
                .repo
                .claim_due(
                    &self.worker_id,
                    &kinds,
                    self.cfg.visibility_timeout,
                    self.cfg.batch_size,
                )
                .await
            {
                Ok(v) => v,
                Err(err) => {
                    tracing::error!(error = %err, "claim_due failed");
                    Vec::new()
                }
            };

            if claimed.is_empty() {
                tokio::select! {
                    _ = tokio::time::sleep(self.cfg.poll_interval) => {}
                    _ = shutdown.changed() => {}
                }
                continue;
            }

            for job in claimed {
                self.dispatch(job).await;
            }
        }
        tracing::info!(worker_id = %self.worker_id, "job runner stopped");
    }

    async fn dispatch(&self, job: Job) {
        let handler = self.handlers.get(job.kind.as_str()).cloned();
        let outcome = match handler {
            None => Err(JobError::Permanent(format!(
                "no handler for kind '{}'",
                job.kind
            ))),
            Some(h) => h.handle(&job.payload).await,
        };

        match outcome {
            Ok(()) => {
                if let Err(err) = self.repo.mark_done(job.id).await {
                    tracing::error!(job_id = %job.id, error = %err, "mark_done failed");
                }
            }
            Err(JobError::Permanent(msg)) => {
                tracing::warn!(job_id = %job.id, kind = %job.kind, error = %msg, "permanent failure");
                if let Err(err) = self.repo.mark_dead(job.id, &msg).await {
                    tracing::error!(job_id = %job.id, error = %err, "mark_dead failed");
                }
            }
            Err(JobError::Retryable(msg)) => {
                let next_attempt = job.attempts + 1;
                if next_attempt >= job.max_attempts {
                    tracing::warn!(
                        job_id = %job.id, kind = %job.kind, attempts = next_attempt,
                        error = %msg, "attempts exhausted, marking dead"
                    );
                    if let Err(err) = self.repo.mark_dead(job.id, &msg).await {
                        tracing::error!(job_id = %job.id, error = %err, "mark_dead failed");
                    }
                } else {
                    let backoff = compute_backoff(
                        next_attempt as u32,
                        self.cfg.backoff_initial,
                        self.cfg.backoff_factor,
                        self.cfg.backoff_max,
                    );
                    let next_run_at = Utc::now()
                        + chrono::Duration::from_std(backoff)
                            .unwrap_or(chrono::Duration::seconds(5));
                    tracing::info!(
                        job_id = %job.id, kind = %job.kind, attempt = next_attempt,
                        backoff_ms = backoff.as_millis() as u64, "retrying"
                    );
                    if let Err(err) = self.repo.mark_failed_retry(job.id, &msg, next_run_at).await {
                        tracing::error!(job_id = %job.id, error = %err, "mark_failed_retry failed");
                    }
                }
            }
        }
    }
}

fn compute_backoff(attempt: u32, initial: Duration, factor: u32, max: Duration) -> Duration {
    // attempt is 1-based (first retry).
    let exp = attempt.saturating_sub(1);
    let mult = (factor as u64).saturating_pow(exp);
    let secs = initial.as_secs().saturating_mul(mult);
    let d = Duration::from_secs(secs);
    if d > max {
        max
    } else {
        d
    }
}

fn hostname() -> Option<String> {
    std::env::var("HOSTNAME").ok().or_else(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let initial = Duration::from_secs(5);
        let factor = 4;
        let max = Duration::from_secs(3600);
        assert_eq!(
            compute_backoff(1, initial, factor, max),
            Duration::from_secs(5)
        );
        assert_eq!(
            compute_backoff(2, initial, factor, max),
            Duration::from_secs(20)
        );
        assert_eq!(
            compute_backoff(3, initial, factor, max),
            Duration::from_secs(80)
        );
        // 5*4^5 = 5120 > 3600 → capped
        assert_eq!(compute_backoff(6, initial, factor, max), max);
    }
}
