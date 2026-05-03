use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::security::model::ServiceAccount;

#[derive(Debug, thiserror::Error)]
pub enum ServiceAccountRepoError {
    #[error("service account name already used in this organisation")]
    DuplicateName,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct NewServiceAccount {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Uuid,
}

#[async_trait]
pub trait ServiceAccountRepository: Send + Sync + 'static {
    /// Atomic insert: users row (kind='service_account') + service_accounts row.
    /// Maps `(organisation_id, name)` UNIQUE violation to `DuplicateName`.
    async fn create(
        &self,
        req: NewServiceAccount,
    ) -> Result<ServiceAccount, ServiceAccountRepoError>;

    async fn find(
        &self,
        organisation_id: Uuid,
        sa_user_id: Uuid,
    ) -> anyhow::Result<Option<ServiceAccount>>;

    /// Returns up to `limit` SAs in the given org, ordered by `(created_at, user_id)`.
    /// `after` paginates: rows strictly after the cursor are returned.
    async fn list(
        &self,
        organisation_id: Uuid,
        limit: u32,
        after: Option<(DateTime<Utc>, Uuid)>,
    ) -> anyhow::Result<Vec<ServiceAccount>>;

    /// Deletes the users row; ON DELETE CASCADE collapses sidecar + keys.
    /// Returns `true` if a row was removed.
    async fn delete(&self, organisation_id: Uuid, sa_user_id: Uuid) -> anyhow::Result<bool>;

    /// Throttled to ≤ 1/min.
    async fn touch_last_used(&self, sa_user_id: Uuid) -> anyhow::Result<()>;
}
