use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct DeleteServiceAccountInput {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteServiceAccountError {
    #[error("service account not found")]
    NotFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn delete_service_account(
    state: &AppState,
    input: DeleteServiceAccountInput,
) -> Result<(), DeleteServiceAccountError> {
    let removed = state
        .service_accounts
        .delete(input.organisation_id, input.sa_user_id)
        .await?;
    if !removed {
        return Err(DeleteServiceAccountError::NotFound);
    }

    let _ = state
        .audit_recorder
        .record(AuditEvent::service_account_deleted(
            input.actor_user_id,
            input.actor_org_id,
            input.sa_user_id,
            input.organisation_id,
        ))
        .await;

    Ok(())
}
