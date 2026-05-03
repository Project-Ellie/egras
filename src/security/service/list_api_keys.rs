use uuid::Uuid;

use crate::app_state::AppState;
use crate::security::model::ApiKey;

#[derive(Debug, Clone)]
pub struct ListApiKeysInput {
    pub organisation_id: Uuid,
    pub sa_user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum ListApiKeysError {
    #[error("service account not found")]
    NotFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn list_api_keys(
    state: &AppState,
    input: ListApiKeysInput,
) -> Result<Vec<ApiKey>, ListApiKeysError> {
    if state
        .service_accounts
        .find(input.organisation_id, input.sa_user_id)
        .await?
        .is_none()
    {
        return Err(ListApiKeysError::NotFound);
    }
    Ok(state.api_keys.list_by_sa(input.sa_user_id).await?)
}
