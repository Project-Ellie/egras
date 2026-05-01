use chrono::{DateTime, Utc};
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, thiserror::Error)]
pub enum LogoutError {
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn logout(
    state: &AppState,
    user_id: Uuid,
    org_id: Uuid,
    jti: Uuid,
    token_expires_at: DateTime<Utc>,
) -> Result<(), LogoutError> {
    state
        .tokens
        .revoke(jti, user_id, token_expires_at)
        .await
        .map_err(|e| LogoutError::Internal(e.into()))?;

    let event = AuditEvent::logout(user_id, org_id, jti);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for logout");
    }
    Ok(())
}
