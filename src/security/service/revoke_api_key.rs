use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct RevokeApiKeyInput {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
    pub key_id: Uuid,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RevokeApiKeyError {
    #[error("service account not found")]
    NotFound,
    #[error("api key not found or already revoked")]
    KeyNotFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn revoke_api_key(
    state: &AppState,
    input: RevokeApiKeyInput,
) -> Result<(), RevokeApiKeyError> {
    if state
        .service_accounts
        .find(input.organisation_id, input.sa_user_id)
        .await?
        .is_none()
    {
        return Err(RevokeApiKeyError::NotFound);
    }

    let revoked = state
        .api_keys
        .revoke(input.sa_user_id, input.key_id)
        .await?;
    if !revoked {
        return Err(RevokeApiKeyError::KeyNotFound);
    }

    let _ = state
        .audit_recorder
        .record(AuditEvent::api_key_revoked(
            input.actor_user_id,
            input.actor_org_id,
            input.sa_user_id,
            input.organisation_id,
            input.key_id,
        ))
        .await;

    Ok(())
}
