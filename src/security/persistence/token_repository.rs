use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::PasswordResetToken;

#[derive(Debug, thiserror::Error)]
pub enum TokenRepoError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait TokenRepository: Send + Sync + 'static {
    async fn insert(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<PasswordResetToken, TokenRepoError>;

    async fn find_valid(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordResetToken>, TokenRepoError>;

    async fn consume(&self, token_id: Uuid) -> Result<(), TokenRepoError>;

    async fn count_pending_for_user(&self, user_id: Uuid) -> Result<u64, TokenRepoError>;
}
