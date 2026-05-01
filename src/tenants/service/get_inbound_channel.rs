use uuid::Uuid;

use crate::app_state::AppState;
use crate::tenants::model::InboundChannel;
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, thiserror::Error)]
pub enum GetChannelError {
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn get_inbound_channel(
    state: &AppState,
    organisation_id: Uuid,
    channel_id: Uuid,
) -> Result<InboundChannel, GetChannelError> {
    match state.inbound_channels.get(organisation_id, channel_id).await {
        Ok(ch) => Ok(ch),
        Err(ChannelRepoError::NotFound) => Err(GetChannelError::NotFound),
        Err(e) => Err(GetChannelError::Repo(e)),
    }
}
