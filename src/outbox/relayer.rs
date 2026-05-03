use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::jobs::model::EnqueueRequest;
use crate::jobs::persistence::JobsRepository;
use crate::outbox::model::AppendRequest;
use crate::outbox::persistence::OutboxRepository;

/// Narrow facade for service code: only `append_in_tx`. Services depend on
/// this through `AppState::outbox` so they can co-commit a domain change
/// and an event row in one transaction.
#[async_trait]
pub trait OutboxAppender: Send + Sync + 'static {
    async fn append_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        req: AppendRequest,
    ) -> anyhow::Result<Uuid>;
}

#[derive(Debug, Clone)]
pub struct OutboxRelayerConfig {
    pub poll_interval: Duration,
    pub batch_size: u32,
    /// `max_attempts` applied to jobs enqueued from outbox events.
    pub job_max_attempts: i32,
}

impl Default for OutboxRelayerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(250),
            batch_size: 64,
            job_max_attempts: 5,
        }
    }
}

pub struct OutboxRelayer {
    pool: PgPool,
    outbox: Arc<dyn OutboxRepository>,
    jobs: Arc<dyn JobsRepository>,
    cfg: OutboxRelayerConfig,
}

pub struct OutboxRelayerHandle {
    task: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
}

impl OutboxRelayerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        if let Err(err) = self.task.await {
            tracing::error!(error = %err, "outbox relayer task join error");
        }
    }
}

impl OutboxRelayer {
    pub fn new(
        pool: PgPool,
        outbox: Arc<dyn OutboxRepository>,
        jobs: Arc<dyn JobsRepository>,
        cfg: OutboxRelayerConfig,
    ) -> Self {
        Self {
            pool,
            outbox,
            jobs,
            cfg,
        }
    }

    pub fn spawn(self) -> OutboxRelayerHandle {
        let (tx, rx) = watch::channel(false);
        let task = tokio::spawn(self.run(rx));
        OutboxRelayerHandle { task, shutdown: tx }
    }

    /// Process up to one batch. Returns the number of events relayed.
    pub async fn tick(&self) -> anyhow::Result<usize> {
        let mut tx = self.pool.begin().await?;
        let events = self
            .outbox
            .claim_unrelayed_in_tx(&mut tx, self.cfg.batch_size)
            .await?;
        if events.is_empty() {
            tx.rollback().await.ok();
            return Ok(0);
        }
        let ids: Vec<Uuid> = events.iter().map(|e| e.id).collect();
        for evt in &events {
            self.jobs
                .enqueue_in_tx(
                    &mut tx,
                    EnqueueRequest::now(evt.event_type.clone(), evt.payload.clone())
                        .with_max_attempts(self.cfg.job_max_attempts),
                )
                .await?;
        }
        self.outbox.mark_relayed_in_tx(&mut tx, &ids).await?;
        tx.commit().await?;
        Ok(events.len())
    }

    async fn run(self, mut shutdown: watch::Receiver<bool>) {
        tracing::info!("outbox relayer started");
        loop {
            if *shutdown.borrow() {
                break;
            }
            match self.tick().await {
                Ok(0) => {
                    tokio::select! {
                        _ = tokio::time::sleep(self.cfg.poll_interval) => {}
                        _ = shutdown.changed() => {}
                    }
                }
                Ok(n) => {
                    tracing::debug!(relayed = n, "outbox relayer batch");
                }
                Err(err) => {
                    tracing::error!(error = %err, "outbox relayer tick failed");
                    tokio::select! {
                        _ = tokio::time::sleep(self.cfg.poll_interval) => {}
                        _ = shutdown.changed() => {}
                    }
                }
            }
        }
        tracing::info!("outbox relayer stopped");
    }
}
