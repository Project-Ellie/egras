use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc::Sender;

use crate::audit::model::AuditEvent;

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("audit channel full; event dropped")]
    ChannelFull,
    #[error("audit channel closed; recorder no longer accepts events")]
    Closed,
}

/// Service trait: services inject `Arc<dyn AuditRecorder>` and call `record` at outcome points.
#[async_trait]
pub trait AuditRecorder: Send + Sync + 'static {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError>;
}

/// Production recorder: non-blocking enqueue onto a bounded mpsc.
pub struct ChannelAuditRecorder {
    tx: Sender<AuditEvent>,
}

impl ChannelAuditRecorder {
    pub fn new(tx: Sender<AuditEvent>) -> Self { Self { tx } }
}

#[async_trait]
impl AuditRecorder for ChannelAuditRecorder {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError> {
        // Mirror to structured log regardless of channel state (spec §7.1, §13).
        tracing::info!(
            target: "egras::audit",
            event_id   = %event.id,
            occurred_at = %event.occurred_at,
            category   = event.category.as_str(),
            event_type = %event.event_type,
            outcome    = event.outcome.as_str(),
            reason_code = ?event.reason_code,
            actor_user_id = ?event.actor_user_id,
            actor_org_id  = ?event.actor_organisation_id,
            target_type   = ?event.target_type,
            target_id     = ?event.target_id,
            target_org_id = ?event.target_organisation_id,
            payload       = %event.payload,
            "audit"
        );
        match self.tx.try_send(event) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(e)) => {
                tracing::error!(
                    event_id = %e.id, event_type = %e.event_type,
                    "audit channel full; dropping event"
                );
                Err(RecorderError::ChannelFull)
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(RecorderError::Closed),
        }
    }
}
