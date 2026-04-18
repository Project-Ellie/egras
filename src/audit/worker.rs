use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

use crate::audit::model::AuditEvent;
use crate::audit::persistence::AuditRepository;

pub struct AuditWorker {
    rx: Receiver<AuditEvent>,
    repo: Arc<dyn AuditRepository>,
    max_retries: u32,
    backoff_initial_ms: u64,
}

pub struct AuditWorkerHandle {
    task: JoinHandle<()>,
}

impl AuditWorkerHandle {
    /// Wait for the worker task to complete draining after the sender is dropped/closed.
    pub async fn shutdown(self) {
        if let Err(err) = self.task.await {
            tracing::error!(error = %err, "audit worker task join error");
        }
    }
}

impl AuditWorker {
    pub fn new(
        rx: Receiver<AuditEvent>,
        repo: Arc<dyn AuditRepository>,
        max_retries: u32,
        backoff_initial_ms: u64,
    ) -> Self {
        Self {
            rx,
            repo,
            max_retries,
            backoff_initial_ms,
        }
    }

    pub fn spawn(self) -> AuditWorkerHandle {
        let task = tokio::spawn(self.run());
        AuditWorkerHandle { task }
    }

    async fn run(mut self) {
        tracing::info!("audit worker started");
        while let Some(event) = self.rx.recv().await {
            self.write_with_retry(event).await;
        }
        tracing::info!("audit worker stopped (channel closed, queue drained)");
    }

    async fn write_with_retry(&self, event: AuditEvent) {
        let mut attempt: u32 = 0;
        let mut backoff_ms = self.backoff_initial_ms;
        loop {
            match self.repo.insert(&event).await {
                Ok(()) => return,
                Err(err) => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        tracing::error!(
                            event_id = %event.id,
                            event_type = %event.event_type,
                            attempt,
                            error = %err,
                            payload = %event.payload,
                            "audit worker: permanent failure, dropping event"
                        );
                        return;
                    }
                    tracing::warn!(
                        event_id = %event.id,
                        attempt,
                        error = %err,
                        "audit worker: retryable failure"
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = backoff_ms.saturating_mul(4);
                }
            }
        }
    }
}
