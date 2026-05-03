use async_trait::async_trait;
use uuid::Uuid;

use crate::security::model::ApiKey;

#[derive(Debug, thiserror::Error)]
pub enum ApiKeyRepoError {
    #[error("api key prefix collision; retry")]
    DuplicatePrefix,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct NewApiKeyRow {
    pub id: Uuid,
    pub service_account_user_id: Uuid,
    pub prefix: String,
    pub secret_hash: String,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_by: Uuid,
}

/// Row variant used by the auth verifier — carries `secret_hash` and the
/// joined `organisation_id` so the verifier doesn't need a second round-trip.
#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub key: ApiKey,
    pub secret_hash: String,
    pub organisation_id: Uuid,
}

#[async_trait]
pub trait ApiKeyRepository: Send + Sync + 'static {
    async fn create(&self, req: NewApiKeyRow) -> Result<ApiKey, ApiKeyRepoError>;

    /// Lookup by prefix, joined with `service_accounts` for `organisation_id`.
    /// Returns active (non-revoked) key only.
    async fn find_active_by_prefix(&self, prefix: &str) -> anyhow::Result<Option<ApiKeyRow>>;

    async fn find(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<Option<ApiKey>>;

    async fn list_by_sa(&self, sa_user_id: Uuid) -> anyhow::Result<Vec<ApiKey>>;

    /// Idempotent: returns `true` if it transitioned from active to revoked,
    /// `false` if missing or already revoked.
    async fn revoke(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<bool>;

    /// Throttled to ≤ 1/min/key.
    async fn touch_last_used(&self, key_id: Uuid) -> anyhow::Result<()>;

    /// Atomic create + revoke for `rotate`.
    async fn rotate(&self, old_key_id: Uuid, new: NewApiKeyRow) -> Result<ApiKey, ApiKeyRepoError>;
}
