use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::features::model::{FeatureDefinition, OrgFeatureOverride};

#[derive(Debug, thiserror::Error)]
pub enum FeatureRepoError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[async_trait]
pub trait FeatureRepository: Send + Sync + 'static {
    async fn list_definitions(&self) -> Result<Vec<FeatureDefinition>, FeatureRepoError>;
    async fn get_definition(
        &self,
        slug: &str,
    ) -> Result<Option<FeatureDefinition>, FeatureRepoError>;
    async fn list_overrides_for_org(
        &self,
        org: Uuid,
    ) -> Result<Vec<OrgFeatureOverride>, FeatureRepoError>;
    async fn get_override(
        &self,
        org: Uuid,
        slug: &str,
    ) -> Result<Option<OrgFeatureOverride>, FeatureRepoError>;
    /// Upserts. Returns previous value (if any) for audit.
    async fn upsert_override(
        &self,
        org: Uuid,
        slug: &str,
        value: Value,
        updated_by: Uuid,
    ) -> Result<Option<Value>, FeatureRepoError>;
    /// Deletes if present. Returns previous value (if any) for audit.
    async fn delete_override(
        &self,
        org: Uuid,
        slug: &str,
    ) -> Result<Option<Value>, FeatureRepoError>;
}
