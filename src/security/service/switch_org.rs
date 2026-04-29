use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::auth::jwt::encode_access_token;

#[derive(Debug, Clone)]
pub struct SwitchOrgInput {
    pub target_org_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct SwitchOrgOutput {
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SwitchOrgError {
    #[error("user is not a member of the target organisation")]
    NotMember,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn switch_org(
    state: &AppState,
    user_id: Uuid,
    current_org_id: Uuid,
    input: SwitchOrgInput,
) -> Result<SwitchOrgOutput, SwitchOrgError> {
    let is_member = state
        .organisations
        .is_member(user_id, input.target_org_id)
        .await
        .map_err(|e| SwitchOrgError::Internal(anyhow::anyhow!(e)))?;

    if !is_member {
        return Err(SwitchOrgError::NotMember);
    }

    let token = encode_access_token(
        &state.jwt_config.secret,
        &state.jwt_config.issuer,
        user_id,
        input.target_org_id,
        state.jwt_config.ttl_secs,
    )
    .map_err(SwitchOrgError::Internal)?;

    let event = AuditEvent::session_switched_org(user_id, current_org_id, input.target_org_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for session.switched_org");
    }

    Ok(SwitchOrgOutput { token })
}
