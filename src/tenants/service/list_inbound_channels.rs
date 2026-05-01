use uuid::Uuid;

use crate::app_state::AppState;
use crate::pagination as cursor_codec;
use crate::tenants::model::{ChannelCursor, InboundChannel};
use crate::tenants::persistence::channel_repository::ChannelRepoError;

#[derive(Debug, Clone)]
pub struct ListChannelsInput {
    pub organisation_id: Uuid,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct ListChannelsOutput {
    pub items: Vec<InboundChannel>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListChannelsError {
    #[error("invalid cursor")]
    InvalidCursor,
    #[error(transparent)]
    Repo(#[from] ChannelRepoError),
}

pub async fn list_inbound_channels(
    state: &AppState,
    input: ListChannelsInput,
) -> Result<ListChannelsOutput, ListChannelsError> {
    let after: Option<ChannelCursor> = match input.after {
        Some(ref raw) => Some(
            cursor_codec::decode::<ChannelCursor>(raw)
                .map_err(|_| ListChannelsError::InvalidCursor)?,
        ),
        None => None,
    };

    let limit = input.limit;
    let mut items = state
        .inbound_channels
        .list(input.organisation_id, after, limit + 1)
        .await?;

    let next_cursor = if items.len() as u32 > limit {
        items.truncate(limit as usize);
        items.last().map(|ch| {
            cursor_codec::encode(&ChannelCursor {
                created_at: ch.created_at,
                id: ch.id,
            })
        })
    } else {
        None
    };

    Ok(ListChannelsOutput { items, next_cursor })
}
