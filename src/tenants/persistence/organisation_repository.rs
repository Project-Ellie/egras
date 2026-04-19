use async_trait::async_trait;
use uuid::Uuid;

use crate::tenants::model::{
    MemberSummary, MembershipCursor, Organisation, OrganisationCursor, OrganisationSummary,
};

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("duplicate organisation name: {0}")]
    DuplicateName(String),
    #[error("unknown role code: {0}")]
    UnknownRoleCode(String),
    #[error("unknown user: {0}")]
    UnknownUser(Uuid),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait OrganisationRepository: Send + Sync + 'static {
    async fn create(&self, name: &str, business: &str) -> Result<Organisation, RepoError>;

    /// Create an organisation and assign `owner_role_code` to `creator_user_id`
    /// inside a single transaction. Used by `create_organisation`.
    async fn create_with_initial_owner(
        &self,
        name: &str,
        business: &str,
        creator_user_id: Uuid,
        owner_role_code: &str,
    ) -> Result<Organisation, RepoError>;

    async fn list_for_user(
        &self,
        user_id: Uuid,
        after: Option<OrganisationCursor>,
        limit: u32,
    ) -> Result<Vec<OrganisationSummary>, RepoError>;

    async fn list_members(
        &self,
        organisation_id: Uuid,
        after: Option<MembershipCursor>,
        limit: u32,
    ) -> Result<Vec<MemberSummary>, RepoError>;

    /// Returns true iff `(user_id, organisation_id)` has at least one role row.
    /// Used by the cross-org rule in service layer.
    async fn is_member(&self, user_id: Uuid, organisation_id: Uuid) -> Result<bool, RepoError>;
}
