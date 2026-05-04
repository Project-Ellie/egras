use crate::features::model::FeatureDefinition;
use crate::features::persistence::{FeatureRepoError, FeatureRepository};

pub async fn list_definitions(
    repo: &dyn FeatureRepository,
) -> Result<Vec<FeatureDefinition>, FeatureRepoError> {
    repo.list_definitions().await
}
