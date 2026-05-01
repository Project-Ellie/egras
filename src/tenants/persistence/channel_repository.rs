use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::{ChannelCursor, ChannelType, InboundChannel};

#[derive(Debug, thiserror::Error)]
pub enum ChannelRepoError {
    #[error("duplicate channel name: {0}")]
    DuplicateName(String),
    #[error("channel not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait InboundChannelRepository: Send + Sync + 'static {
    /// Insert a new channel. Generates id and api_key internally.
    async fn create(
        &self,
        organisation_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError>;

    async fn list(
        &self,
        organisation_id: Uuid,
        after: Option<ChannelCursor>,
        limit: u32,
    ) -> Result<Vec<InboundChannel>, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn get(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<InboundChannel, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn update(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError>;

    /// Returns `NotFound` if id doesn't exist or belongs to a different org.
    async fn delete(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<(), ChannelRepoError>;
}
