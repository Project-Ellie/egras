use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::tenants::persistence::RepoError;

#[derive(Debug, Clone)]
pub struct CreateOrganisationInput {
    pub name: String,
    pub business: String,
    pub seed_creator_as_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOrganisationOutput {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateOrganisationError {
    #[error("organisation name already exists")]
    DuplicateName,
    #[error("invalid name: must be non-empty and ≤ 120 chars")]
    InvalidName,
    #[error("invalid business: must be non-empty and ≤ 120 chars")]
    InvalidBusiness,
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn create_organisation(
    state: &AppState,
    creator_user_id: Uuid,
    actor_org: Uuid,
    input: CreateOrganisationInput,
) -> Result<CreateOrganisationOutput, CreateOrganisationError> {
    let name = input.name.trim().to_string();
    let business = input.business.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(CreateOrganisationError::InvalidName);
    }
    if business.is_empty() || business.chars().count() > 120 {
        return Err(CreateOrganisationError::InvalidBusiness);
    }

    let org = if input.seed_creator_as_owner {
        match state
            .organisations
            .create_with_initial_owner(&name, &business, creator_user_id, "org_owner")
            .await
        {
            Ok(o) => o,
            Err(RepoError::DuplicateName(_)) => return Err(CreateOrganisationError::DuplicateName),
            Err(e) => return Err(CreateOrganisationError::Repo(e)),
        }
    } else {
        match state.organisations.create(&name, &business).await {
            Ok(o) => o,
            Err(RepoError::DuplicateName(_)) => return Err(CreateOrganisationError::DuplicateName),
            Err(e) => return Err(CreateOrganisationError::Repo(e)),
        }
    };

    let event = AuditEvent::organisation_created(creator_user_id, actor_org, org.id, &name);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, org_id = %org.id, "audit record failed for organisation.created");
    }

    Ok(CreateOrganisationOutput {
        id: org.id,
        name: org.name,
        business: org.business,
        role_codes: if input.seed_creator_as_owner {
            vec!["org_owner".into()]
        } else {
            vec![]
        },
    })
}
