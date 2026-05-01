use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, thiserror::Error)]
pub enum DeleteChannelError {
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn delete_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    organisation_id: Uuid,
    channel_id: Uuid,
) -> Result<(), DeleteChannelError> {
    match state
        .inbound_channels
        .delete(organisation_id, channel_id)
        .await
    {
        Ok(()) => {}
        Err(ChannelRepoError::NotFound) => return Err(DeleteChannelError::NotFound),
        Err(e) => return Err(DeleteChannelError::Repo(e)),
    }

    let event = AuditEvent::channel_deleted(actor_user_id, actor_org, channel_id, organisation_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %channel_id, "audit record failed for channel.deleted");
    }

    Ok(())
}
