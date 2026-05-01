use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::model::{ChannelType, InboundChannel};
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, Clone)]
pub struct CreateChannelInput {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateChannelError {
    #[error("channel name already taken")]
    DuplicateName,
    #[error("invalid name")]
    InvalidName,
    #[error("invalid description")]
    InvalidDescription,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn create_inbound_channel(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org: Uuid,
    input: CreateChannelInput,
) -> Result<InboundChannel, CreateChannelError> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(CreateChannelError::InvalidName);
    }
    if let Some(ref d) = input.description {
        if d.chars().count() > 1000 {
            return Err(CreateChannelError::InvalidDescription);
        }
    }

    let ch = match state
        .inbound_channels
        .create(
            input.organisation_id,
            &name,
            input.description.as_deref(),
            input.channel_type,
            input.is_active,
        )
        .await
    {
        Ok(ch) => ch,
        Err(ChannelRepoError::DuplicateName(_)) => return Err(CreateChannelError::DuplicateName),
        Err(e) => return Err(CreateChannelError::Repo(e)),
    };

    let event = AuditEvent::channel_created(
        actor_user_id,
        actor_org,
        ch.id,
        ch.organisation_id,
        &ch.name,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, channel_id = %ch.id, "audit record failed for channel.created");
    }

    Ok(ch)
}
