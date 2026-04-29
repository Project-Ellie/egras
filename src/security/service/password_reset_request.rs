use rand::RngCore;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;

#[derive(Debug, Clone)]
pub struct PasswordResetRequestInput {
    pub email: String,
    /// Base URL used when constructing the reset link logged at INFO.
    pub base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PasswordResetRequestError {
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn password_reset_request(
    state: &AppState,
    input: PasswordResetRequestInput,
) -> Result<(), PasswordResetRequestError> {
    // Always emit audit so timing is uniform.
    let event = AuditEvent::password_reset_requested(&input.email);
    if let Err(e) = state.audit_recorder.record(event).await {
        warn!(error = %e, "audit record failed for password.reset_requested");
    }

    let user = state
        .users
        .find_by_username_or_email(&input.email)
        .await
        .map_err(|e| PasswordResetRequestError::Internal(e.into()))?;

    let Some(user) = user else {
        // Return success silently — do not leak whether the email exists.
        return Ok(());
    };

    // Generate 32 random bytes; raw = hex string; stored = SHA-256(raw).
    let mut raw_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw_bytes);
    let raw_hex = hex::encode(raw_bytes);
    let token_hash = hex::encode(Sha256::digest(raw_bytes));

    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(state.password_reset_ttl_secs);

    state
        .tokens
        .insert(user.id, &token_hash, expires_at)
        .await
        .map_err(|e| PasswordResetRequestError::Internal(e.into()))?;

    // Log reset URL at INFO (email delivery is out of scope for this seed).
    info!(
        user_id = %user.id,
        reset_url = %format!("{}/reset-password?token={}", input.base_url.trim_end_matches('/'), raw_hex),
        "password reset token issued",
    );

    Ok(())
}
