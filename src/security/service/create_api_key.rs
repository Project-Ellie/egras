use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::security::model::ApiKeyMaterial;
use crate::security::persistence::{ApiKeyRepoError, NewApiKeyRow};
use crate::security::service::api_key_secret;

#[derive(Debug, Clone)]
pub struct CreateApiKeyInput {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
    pub name: String,
    /// `None` = inherit all permissions of the SA. `Some(empty)` is rejected.
    pub scopes: Option<Vec<String>>,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateApiKeyError {
    #[error("service account not found")]
    NotFound,
    #[error("scopes cannot be empty (use null to inherit)")]
    EmptyScopes,
    #[error("could not allocate unique key prefix; please retry")]
    PrefixCollision,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn create_api_key(
    state: &AppState,
    input: CreateApiKeyInput,
) -> Result<ApiKeyMaterial, CreateApiKeyError> {
    if matches!(&input.scopes, Some(s) if s.is_empty()) {
        return Err(CreateApiKeyError::EmptyScopes);
    }
    if state
        .service_accounts
        .find(input.organisation_id, input.sa_user_id)
        .await?
        .is_none()
    {
        return Err(CreateApiKeyError::NotFound);
    }

    // One retry on the (~1 in 4 B) prefix collision.
    for _ in 0..2 {
        let g = api_key_secret::generate()?;
        let row = NewApiKeyRow {
            id: Uuid::now_v7(),
            service_account_user_id: input.sa_user_id,
            prefix: g.prefix.clone(),
            secret_hash: api_key_secret::hash_secret(&g.secret)?,
            name: input.name.clone(),
            scopes: input.scopes.clone(),
            created_by: input.actor_user_id,
        };
        match state.api_keys.create(row).await {
            Ok(key) => {
                let _ = state
                    .audit_recorder
                    .record(AuditEvent::api_key_created(
                        input.actor_user_id,
                        input.actor_org_id,
                        input.sa_user_id,
                        input.organisation_id,
                        key.id,
                        &key.prefix,
                    ))
                    .await;
                return Ok(ApiKeyMaterial {
                    key,
                    plaintext: g.plaintext,
                });
            }
            Err(ApiKeyRepoError::DuplicatePrefix) => continue,
            Err(ApiKeyRepoError::Other(e)) => return Err(CreateApiKeyError::Other(e)),
        }
    }
    Err(CreateApiKeyError::PrefixCollision)
}
