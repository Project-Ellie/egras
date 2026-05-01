use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::{User, UserCursor, UserMembership};

#[derive(Debug, thiserror::Error)]
pub enum CreateAndAddError {
    #[error("duplicate username: {0}")]
    DuplicateUsername(String),
    #[error("duplicate email: {0}")]
    DuplicateEmail(String),
    #[error("organisation not found")]
    OrgNotFound,
    #[error("unknown role code: {0}")]
    UnknownRoleCode(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

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

    async fn list_memberships(&self, user_id: Uuid) -> Result<Vec<UserMembership>, UserRepoError>;

    async fn list_users(
        &self,
        org_id: Option<Uuid>,
        q: Option<&str>,
        cursor: Option<UserCursor>,
        limit: u32,
    ) -> Result<Vec<User>, UserRepoError>;

    async fn list_memberships_for_users(
        &self,
        user_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, UserMembership)>, UserRepoError>;

    /// Create a user and add them to `org_id` with `role_code` atomically.
    /// On failure no partial state is left in the database.
    async fn create_and_add_to_org(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
        org_id: Uuid,
        role_code: &str,
    ) -> Result<User, CreateAndAddError>;
}
