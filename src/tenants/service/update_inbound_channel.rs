use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::model::{ChannelType, InboundChannel};
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, Clone)]
pub struct UpdateChannelInput {
    pub organisation_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateChannelError {
    #[error("channel not found")]
    NotFound,
    #[error("channel name already taken")]
    DuplicateName,
    #[error("invalid name")]
    InvalidName,
    #[error("invalid description")]
    InvalidDescription,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn update_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    input: UpdateChannelInput,
) -> Result<InboundChannel, UpdateChannelError> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(UpdateChannelError::InvalidName);
    }
    if let Some(ref d) = input.description {
        if d.chars().count() > 1000 {
            return Err(UpdateChannelError::InvalidDescription);
        }
    }

    let ch = match state
        .inbound_channels
        .update(
            input.organisation_id,
            input.channel_id,
            &name,
            input.description.as_deref(),
            input.channel_type,
            input.is_active,
        )
        .await
    {
        Ok(ch) => ch,
        Err(ChannelRepoError::NotFound) => return Err(UpdateChannelError::NotFound),
        Err(ChannelRepoError::DuplicateName(_)) => return Err(UpdateChannelError::DuplicateName),
        Err(e) => return Err(UpdateChannelError::Repo(e)),
    };

    let event = AuditEvent::channel_updated(
        actor_user_id,
        actor_org,
        ch.id,
        ch.organisation_id,
        &ch.name,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %ch.id, "audit record failed for channel.updated");
    }

    Ok(ch)
}
