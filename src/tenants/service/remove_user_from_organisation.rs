use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct RemoveUserFromOrganisationInput {
    pub user_id: Uuid,
    pub org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveUserFromOrganisationError {
    #[error("user is not a member of the organisation")]
    NotMember,
    #[error("cannot remove the last owner of an organisation")]
    LastOwner,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn remove_user_from_organisation(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: RemoveUserFromOrganisationInput,
) -> Result<(), RemoveUserFromOrganisationError> {
    state
        .organisations
        .remove_member_checked(input.user_id, input.org_id)
        .await
        .map_err(|e| match e {
            RepoError::NotMember => RemoveUserFromOrganisationError::NotMember,
            RepoError::LastOwner => RemoveUserFromOrganisationError::LastOwner,
            e => RemoveUserFromOrganisationError::Repo(e),
        })?;

    let event = AuditEvent::organisation_member_removed(
        actor_user_id,
        actor_org_id,
        input.org_id,
        input.user_id,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for organisation.member_removed");
    }

    Ok(())
}
