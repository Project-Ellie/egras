use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::security::model::ServiceAccount;
use crate::security::persistence::{NewServiceAccount, ServiceAccountRepoError};

#[derive(Debug, Clone)]
pub struct CreateServiceAccountInput {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateServiceAccountError {
    #[error("name already used in this organisation")]
    DuplicateName,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn create_service_account(
    state: &AppState,
    input: CreateServiceAccountInput,
) -> Result<ServiceAccount, CreateServiceAccountError> {
    let sa = state
        .service_accounts
        .create(NewServiceAccount {
            organisation_id: input.organisation_id,
            name: input.name.clone(),
            description: input.description.clone(),
            created_by: input.actor_user_id,
        })
        .await
        .map_err(|e| match e {
            ServiceAccountRepoError::DuplicateName => CreateServiceAccountError::DuplicateName,
            ServiceAccountRepoError::Other(other) => CreateServiceAccountError::Other(other),
        })?;

    let _ = state
        .audit_recorder
        .record(AuditEvent::service_account_created(
            input.actor_user_id,
            input.actor_org_id,
            sa.user_id,
            input.organisation_id,
            &input.name,
        ))
        .await;

    Ok(sa)
}
