use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct ChangePasswordInput {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ChangePasswordError {
    #[error("current password is incorrect")]
    WrongCurrentPassword,
    #[error("new password too short (min 8 chars)")]
    PasswordTooShort,
    #[error("user not found")]
    UserNotFound,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn change_password(
    state: &AppState,
    user_id: Uuid,
    input: ChangePasswordInput,
) -> Result<(), ChangePasswordError> {
    if input.new_password.len() < 8 {
        return Err(ChangePasswordError::PasswordTooShort);
    }

    let user = state
        .users
        .find_by_id(user_id)
        .await
        .map_err(|e| ChangePasswordError::Internal(e.into()))?
        .ok_or(ChangePasswordError::UserNotFound)?;

    let ok = super::password_hash::verify_password(&input.current_password, &user.password_hash)
        .map_err(ChangePasswordError::Internal)?;
    if !ok {
        return Err(ChangePasswordError::WrongCurrentPassword);
    }

    let new_hash = super::password_hash::hash_password(&input.new_password)
        .map_err(ChangePasswordError::Internal)?;

    state
        .users
        .update_password_hash(user_id, &new_hash)
        .await
        .map_err(|e| ChangePasswordError::Internal(e.into()))?;

    let event = AuditEvent::password_changed(user_id);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %user_id, "audit record failed for password.changed");
    }

    Ok(())
}
