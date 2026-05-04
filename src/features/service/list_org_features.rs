use uuid::Uuid;

use crate::features::model::{EvaluatedFeature, FeatureSource};
use crate::features::persistence::{FeatureRepoError, FeatureRepository};
use crate::features::service::evaluate::FeatureEvaluator;

#[derive(Debug, thiserror::Error)]
pub enum ListOrgFeaturesError {
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<FeatureRepoError> for ListOrgFeaturesError {
    fn from(e: FeatureRepoError) -> Self {
        Self::Other(anyhow::anyhow!("{e}"))
    }
}

pub async fn list_org_features(
    repo: &dyn FeatureRepository,
    evaluator: &dyn FeatureEvaluator,
    org_id: Uuid,
) -> Result<Vec<EvaluatedFeature>, ListOrgFeaturesError> {
    let definitions = repo.list_definitions().await?;
    let overrides = repo.list_overrides_for_org(org_id).await?;

    let mut results = Vec::with_capacity(definitions.len());

    for def in definitions {
        let source = if overrides.iter().any(|ov| ov.slug == def.slug) {
            FeatureSource::Override
        } else {
            FeatureSource::Default
        };

        // Use the evaluator so the cache is authoritative for the value.
        let value = evaluator
            .evaluate(org_id, &def.slug)
            .await
            .map_err(|e| ListOrgFeaturesError::Other(anyhow::anyhow!("{e}")))?;

        results.push(EvaluatedFeature {
            slug: def.slug,
            value,
            source,
            value_type: def.value_type,
            self_service: def.self_service,
        });
    }

    Ok(results)
}
