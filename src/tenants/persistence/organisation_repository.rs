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
    #[error("organisation or user not found")]
    NotFound,
    #[error("user is not a member of the organisation")]
    NotMember,
    #[error("cannot remove the last owner of an organisation")]
    LastOwner,
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

    /// Add a user to an org with the given role_code. Idempotent on the role row.
    async fn add_member(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        role_code: &str,
    ) -> Result<(), RepoError>;

    /// Remove all role rows for (user_id, org_id). Refuses with `LastOwner`
    /// if this would leave the org with zero org_owner rows. Uses FOR UPDATE.
    async fn remove_member_checked(&self, user_id: Uuid, org_id: Uuid) -> Result<(), RepoError>;
}
