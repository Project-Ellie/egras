use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::Role;
use crate::tenants::persistence::organisation_repository::RepoError;

#[async_trait]
pub trait RoleRepository: Send + Sync + 'static {
    async fn find_by_code(&self, code: &str) -> Result<Option<Role>, RepoError>;

    /// Idempotent: a row already matching `(user, org, role)` is a no-op, not a
    /// conflict. `UnknownUser` / `UnknownRoleCode` map to `RepoError::UnknownUser`
    /// / `RepoError::UnknownRoleCode` rather than a raw FK violation.
    async fn assign(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), RepoError>;
}
