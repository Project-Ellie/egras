use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct AddUserToOrganisationInput {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AddUserToOrganisationError {
    #[error("organisation or user not found")]
    NotFound,
    #[error("unknown role code")]
    UnknownRoleCode,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn add_user_to_organisation(
    state: &AppState,
    actor_user_id: Uuid,
    actor_org_id: Uuid,
    input: AddUserToOrganisationInput,
) -> Result<(), AddUserToOrganisationError> {
    state
        .organisations
        .add_member(input.user_id, input.org_id, &input.role_code)
        .await
        .map_err(|e| match e {
            RepoError::NotFound => AddUserToOrganisationError::NotFound,
            RepoError::UnknownRoleCode(_) => AddUserToOrganisationError::UnknownRoleCode,
            e => AddUserToOrganisationError::Repo(e),
        })?;

    let event = AuditEvent::organisation_member_added(
        actor_user_id,
        actor_org_id,
        input.org_id,
        input.user_id,
        &input.role_code,
    );
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for organisation.member_added");
    }

    Ok(())
}
