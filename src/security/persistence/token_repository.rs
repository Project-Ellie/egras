use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
        expires_at: DateTime<Utc>,
    ) -> Result<PasswordResetToken, TokenRepoError>;

    async fn find_valid(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordResetToken>, TokenRepoError>;

    async fn consume(&self, token_id: Uuid) -> Result<(), TokenRepoError>;

    async fn count_pending_for_user(&self, user_id: Uuid) -> Result<i64, TokenRepoError>;

    /// Record a revoked JWT so it cannot be reused before its natural expiry.
    async fn revoke(
        &self,
        jti: Uuid,
        user_id: Uuid,
        expires_at: DateTime<Utc>,
    ) -> Result<(), TokenRepoError>;

    /// Returns true if the given JTI has been revoked and has not yet expired.
    async fn is_revoked(&self, jti: Uuid) -> Result<bool, TokenRepoError>;
}
