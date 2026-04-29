use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::auth::jwt::encode_access_token;
use crate::security::model::UserMembership;

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub username_or_email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct LoginOutput {
    pub token: String,
    pub user_id: Uuid,
    pub active_org_id: Uuid,
    pub memberships: Vec<UserMembership>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("user belongs to no organisation")]
    NoOrganisation,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn login(state: &AppState, input: LoginInput) -> Result<LoginOutput, LoginError> {
    let user = state
        .users
        .find_by_username_or_email(&input.username_or_email)
        .await
        .map_err(|e| LoginError::Internal(e.into()))?;

    let user = match user {
        Some(u) => u,
        None => {
            let event = AuditEvent::login_failed("not_found", &input.username_or_email);
            if let Err(e) = state.audit_recorder.record(event).await {
                warn!(error = %e, "audit record failed for login.failed");
            }
            return Err(LoginError::InvalidCredentials);
        }
    };

    let ok = super::password_hash::verify_password(&input.password, &user.password_hash)
        .map_err(LoginError::Internal)?;
    if !ok {
        let event = AuditEvent::login_failed("bad_password", &input.username_or_email);
        if let Err(e) = state.audit_recorder.record(event).await {
            warn!(error = %e, "audit record failed for login.failed");
        }
        return Err(LoginError::InvalidCredentials);
    }

    // Opportunistic rehash.
    if super::password_hash::needs_rehash(&user.password_hash) {
        if let Ok(new_hash) = super::password_hash::hash_password(&input.password) {
            let _ = state.users.update_password_hash(user.id, &new_hash).await;
        }
    }

    let memberships = state
        .users
        .list_memberships(user.id)
        .await
        .map_err(|e| LoginError::Internal(e.into()))?;

    if memberships.is_empty() {
        return Err(LoginError::NoOrganisation);
    }

    let active_org_id = memberships[0].org_id;

    let token = encode_access_token(
        &state.jwt_config.secret,
        &state.jwt_config.issuer,
        user.id,
        active_org_id,
        state.jwt_config.ttl_secs,
    )
    .map_err(LoginError::Internal)?;

    let event = AuditEvent::login_success(user.id, active_org_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user.id, "audit record failed for login.success");
    }

    Ok(LoginOutput {
        token,
        user_id: user.id,
        active_org_id,
        memberships,
    })
}
