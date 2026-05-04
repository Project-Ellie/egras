use uuid::Uuid;

use crate::audit::model::AuditEvent;
use crate::audit::service::AuditRecorder;
use crate::features::persistence::{FeatureRepoError, FeatureRepository};
use crate::features::service::evaluate::FeatureEvaluator;

#[derive(Debug, Clone)]
pub struct ClearOrgFeatureInput {
    pub organisation_id: Uuid,
    pub slug: String,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
    pub actor_is_operator: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClearOrgFeatureError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error("flag is not self_service; operator privileges required")]
    NotSelfService,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<FeatureRepoError> for ClearOrgFeatureError {
    fn from(e: FeatureRepoError) -> Self {
        Self::Other(anyhow::anyhow!("{e}"))
    }
}

pub async fn clear_org_feature(
    repo: &dyn FeatureRepository,
    evaluator: &dyn FeatureEvaluator,
    audit: &dyn AuditRecorder,
    input: ClearOrgFeatureInput,
) -> Result<(), ClearOrgFeatureError> {
    // Step 1: Load definition; UnknownSlug if missing.
    let definition = repo
        .get_definition(&input.slug)
        .await?
        .ok_or(ClearOrgFeatureError::UnknownSlug)?;

    // Step 2: Self-service guard (symmetric with set).
    if !definition.self_service && !input.actor_is_operator {
        return Err(ClearOrgFeatureError::NotSelfService);
    }

    // Step 3: Delete override (returns previous value for audit).
    let old_value = repo
        .delete_override(input.organisation_id, &input.slug)
        .await?;

    // Step 4: Invalidate cache.
    evaluator
        .invalidate(input.organisation_id, &input.slug)
        .await;

    // Step 5: Audit.
    let _ = audit
        .record(AuditEvent::feature_cleared(
            input.actor_user_id,
            input.actor_org_id,
            input.organisation_id,
            &input.slug,
            old_value.as_ref(),
            definition.self_service,
        ))
        .await;

    Ok(())
}
