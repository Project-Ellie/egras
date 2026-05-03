use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::security::model::ApiKeyMaterial;
use crate::security::persistence::{ApiKeyRepoError, NewApiKeyRow};
use crate::security::service::api_key_secret;

#[derive(Debug, Clone)]
pub struct RotateApiKeyInput {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
    pub old_key_id: Uuid,
    /// Optional override; defaults to the existing key's name.
    pub name: Option<String>,
    /// Optional override; defaults to the existing key's scopes.
    /// `Some(empty)` is rejected (use `None` to inherit).
    pub scopes: Option<Option<Vec<String>>>,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum RotateApiKeyError {
    #[error("service account not found")]
    NotFound,
    #[error("api key not found")]
    KeyNotFound,
    #[error("scopes cannot be empty (use null to inherit)")]
    EmptyScopes,
    #[error("could not allocate unique key prefix; please retry")]
    PrefixCollision,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn rotate_api_key(
    state: &AppState,
    input: RotateApiKeyInput,
) -> Result<ApiKeyMaterial, RotateApiKeyError> {
    if state
        .service_accounts
        .find(input.organisation_id, input.sa_user_id)
        .await?
        .is_none()
    {
        return Err(RotateApiKeyError::NotFound);
    }

    let old = state
        .api_keys
        .find(input.sa_user_id, input.old_key_id)
        .await?
        .ok_or(RotateApiKeyError::KeyNotFound)?;

    let new_name = input.name.clone().unwrap_or_else(|| old.name.clone());
    let new_scopes = match input.scopes.clone() {
        Some(s) => {
            if matches!(&s, Some(v) if v.is_empty()) {
                return Err(RotateApiKeyError::EmptyScopes);
            }
            s
        }
        None => old.scopes.clone(),
    };

    for _ in 0..2 {
        let g = api_key_secret::generate()?;
        let row = NewApiKeyRow {
            id: Uuid::now_v7(),
            service_account_user_id: input.sa_user_id,
            prefix: g.prefix.clone(),
            secret_hash: api_key_secret::hash_secret(&g.secret)?,
            name: new_name.clone(),
            scopes: new_scopes.clone(),
            created_by: input.actor_user_id,
        };
        match state.api_keys.rotate(input.old_key_id, row).await {
            Ok(key) => {
                let _ = state
                    .audit_recorder
                    .record(AuditEvent::api_key_rotated(
                        input.actor_user_id,
                        input.actor_org_id,
                        input.sa_user_id,
                        input.organisation_id,
                        input.old_key_id,
                        key.id,
                    ))
                    .await;
                return Ok(ApiKeyMaterial {
                    key,
                    plaintext: g.plaintext,
                });
            }
            Err(ApiKeyRepoError::DuplicatePrefix) => continue,
            Err(ApiKeyRepoError::Other(e)) => return Err(RotateApiKeyError::Other(e)),
        }
    }
    Err(RotateApiKeyError::PrefixCollision)
}
