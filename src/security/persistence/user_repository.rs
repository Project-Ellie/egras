use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::{User, UserMembership};

#[derive(Debug, thiserror::Error)]
pub enum UserRepoError {
    #[error("duplicate username: {0}")]
    DuplicateUsername(String),
    #[error("duplicate email: {0}")]
    DuplicateEmail(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait UserRepository: Send + Sync + 'static {
    async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<User, UserRepoError>;

    async fn find_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<User>, UserRepoError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, UserRepoError>;

    async fn update_password_hash(
        &self,
        user_id: Uuid,
        new_hash: &str,
    ) -> Result<(), UserRepoError>;

    async fn list_memberships(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserMembership>, UserRepoError>;
}
