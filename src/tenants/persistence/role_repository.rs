use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::Role;
use crate::tenants::persistence::organisation_repository::RepoError;

#[async_trait]
pub trait RoleRepository: Send + Sync + 'static {
    async fn find_by_code(&self, code: &str) -> Result<Option<Role>, RepoError>;

    /// Idempotent: a row already matching `(user, org, role)` is a no-op, not a
    /// conflict. A missing user is mapped to `RepoError::UnknownUser`; all other
    /// FK failures surface as `RepoError::Db` — the service layer owns
    /// code-to-id resolution before calling this method.
    async fn assign(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), RepoError>;

    /// Returns true iff `(user_id, organisation_id, role_id)` already exists in
    /// `user_organisation_roles`. Used by the service layer for idempotency
    /// detection before calling `assign`.
    async fn has_role(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<bool, RepoError>;
}
