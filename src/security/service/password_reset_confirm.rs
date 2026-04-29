use sha2::{Digest, Sha256};
use tracing::warn;

use crate::app_state::AppState;
use crate::audit::model::{AuditEvent, Outcome};

#[derive(Debug, Clone)]
pub struct PasswordResetConfirmInput {
    /// The raw hex token from the URL (unhashed).
    pub raw_token: String,
    pub new_password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PasswordResetConfirmError {
    #[error("token is invalid or expired")]
    InvalidToken,
    #[error("new password too short (min 8 chars)")]
    PasswordTooShort,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn password_reset_confirm(
    state: &AppState,
    input: PasswordResetConfirmInput,
) -> Result<(), PasswordResetConfirmError> {
    if input.new_password.len() < 8 {
        return Err(PasswordResetConfirmError::PasswordTooShort);
    }

    // Decode raw token bytes and hash them.
    let raw_bytes =
        hex::decode(&input.raw_token).map_err(|_| PasswordResetConfirmError::InvalidToken)?;
    let token_hash = hex::encode(Sha256::digest(&raw_bytes));

    let token = state
        .tokens
        .find_valid(&token_hash)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    let Some(token) = token else {
        let event = AuditEvent::password_reset_confirmed(
            None,
            Outcome::Failure,
            Some("invalid_token".into()),
        );
        if let Err(e) = state.audit_recorder.record(event).await {
            warn!(error = %e, "audit record failed for password.reset_confirmed (invalid)");
        }
        return Err(PasswordResetConfirmError::InvalidToken);
    };

    let new_hash = super::password_hash::hash_password(&input.new_password)
        .map_err(PasswordResetConfirmError::Internal)?;

    state
        .users
        .update_password_hash(token.user_id, &new_hash)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    state
        .tokens
        .consume(token.id)
        .await
        .map_err(|e| PasswordResetConfirmError::Internal(e.into()))?;

    let event = AuditEvent::password_reset_confirmed(Some(token.user_id), Outcome::Success, None);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, user_id = %token.user_id, "audit record failed for password.reset_confirmed");
    }

    Ok(())
}
