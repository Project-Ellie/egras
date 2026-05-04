use serde_json::Value;
use uuid::Uuid;

use crate::audit::model::AuditEvent;
use crate::audit::service::AuditRecorder;
use crate::features::persistence::{FeatureRepoError, FeatureRepository};
use crate::features::service::evaluate::FeatureEvaluator;

#[derive(Debug, Clone)]
pub struct SetOrgFeatureInput {
    pub organisation_id: Uuid,
    pub slug: String,
    pub value: Value,
    pub actor_user_id: Uuid,
    pub actor_org_id: Uuid,
    pub actor_is_operator: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SetOrgFeatureError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error("flag is not self_service; operator privileges required")]
    NotSelfService,
    #[error("value does not match declared type: {0}")]
    InvalidValue(&'static str),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<FeatureRepoError> for SetOrgFeatureError {
    fn from(e: FeatureRepoError) -> Self {
        Self::Other(anyhow::anyhow!("{e}"))
    }
}

pub async fn set_org_feature(
    repo: &dyn FeatureRepository,
    evaluator: &dyn FeatureEvaluator,
    audit: &dyn AuditRecorder,
    input: SetOrgFeatureInput,
) -> Result<(), SetOrgFeatureError> {
    // Step 1: Load definition; UnknownSlug if missing.
    let definition = repo
        .get_definition(&input.slug)
        .await?
        .ok_or(SetOrgFeatureError::UnknownSlug)?;

    // Step 2: Validate value against value_type.
    if let Err(reason) = definition.value_type.validate(&input.value) {
        return Err(SetOrgFeatureError::InvalidValue(reason));
    }

    // Step 3: Self-service guard.
    if !definition.self_service && !input.actor_is_operator {
        return Err(SetOrgFeatureError::NotSelfService);
    }

    // Step 4: Upsert override (returns old value for audit).
    let old_value = repo
        .upsert_override(
            input.organisation_id,
            &input.slug,
            input.value.clone(),
            input.actor_user_id,
        )
        .await?;

    // Step 5: Invalidate cache.
    evaluator
        .invalidate(input.organisation_id, &input.slug)
        .await;

    // Step 6: Audit.
    let _ = audit
        .record(AuditEvent::feature_set(
            input.actor_user_id,
            input.actor_org_id,
            input.organisation_id,
            &input.slug,
            old_value.as_ref(),
            &input.value,
            definition.self_service,
        ))
        .await;

    Ok(())
}
