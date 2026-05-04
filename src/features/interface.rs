use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::auth::extractors::{AuthedCaller, FeaturesManage, FeaturesRead, Perm, TenantsManageAll};
use crate::errors::AppError;
use crate::features::model::{EvaluatedFeature, FeatureDefinition};
use crate::features::service::{
    clear_org_feature, list_definitions, list_org_features, set_org_feature, ClearOrgFeatureError,
    ClearOrgFeatureInput, SetOrgFeatureError, SetOrgFeatureInput,
};

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_definitions))
        .route("/orgs/:org_id", get(get_org_features))
        .route(
            "/orgs/:org_id/:slug",
            put(put_org_feature).delete(delete_org_feature),
        )
}

// ── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct PutFeatureRequest {
    #[schema(value_type = Object)]
    pub value: serde_json::Value,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/v1/features",
    tag = "features",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Feature catalog (operator only)", body = Vec<FeatureDefinition>),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied", body = crate::errors::ErrorBody),
    ),
)]
pub async fn get_definitions(
    State(state): State<AppState>,
    _perm: Perm<TenantsManageAll>,
) -> Result<Json<Vec<FeatureDefinition>>, AppError> {
    let defs = list_definitions(state.features.as_ref())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(Json(defs))
}

#[utoipa::path(
    get,
    path = "/api/v1/features/orgs/{org_id}",
    tag = "features",
    security(("bearer" = [])),
    params(("org_id" = Uuid, Path, description = "Organisation ID")),
    responses(
        (status = 200, description = "Evaluated features for the org", body = Vec<EvaluatedFeature>),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied", body = crate::errors::ErrorBody),
        (status = 404, description = "Organisation not found", body = crate::errors::ErrorBody),
    ),
)]
pub async fn get_org_features(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<FeaturesRead>,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
) -> Result<Json<Vec<EvaluatedFeature>>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    let features = list_org_features(
        state.features.as_ref(),
        state.feature_evaluator.as_ref(),
        org_id,
    )
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(Json(features))
}

#[utoipa::path(
    put,
    path = "/api/v1/features/orgs/{org_id}/{slug}",
    tag = "features",
    request_body = PutFeatureRequest,
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("slug" = String, Path, description = "Feature slug"),
    ),
    responses(
        (status = 200, description = "Updated evaluated feature", body = EvaluatedFeature),
        (status = 400, description = "Invalid value", body = crate::errors::ErrorBody),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied or not self-service", body = crate::errors::ErrorBody),
        (status = 404, description = "Organisation or feature not found", body = crate::errors::ErrorBody),
    ),
)]
pub async fn put_org_feature(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<FeaturesManage>,
    axum::extract::Path((org_id, slug)): axum::extract::Path<(Uuid, String)>,
    Json(req): Json<PutFeatureRequest>,
) -> Result<Json<EvaluatedFeature>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    set_org_feature(
        state.features.as_ref(),
        state.feature_evaluator.as_ref(),
        state.audit_recorder.as_ref(),
        SetOrgFeatureInput {
            organisation_id: org_id,
            slug: slug.clone(),
            value: req.value,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
            actor_is_operator: caller.permissions.is_operator_over_tenants(),
        },
    )
    .await
    .map_err(|e| match e {
        SetOrgFeatureError::UnknownSlug => AppError::FeatureUnknown,
        SetOrgFeatureError::NotSelfService => AppError::FeatureNotSelfService,
        SetOrgFeatureError::InvalidValue(reason) => AppError::FeatureInvalidValue(reason.into()),
        SetOrgFeatureError::Other(e) => AppError::Internal(e),
    })?;

    // Return the newly evaluated state.
    let features = list_org_features(
        state.features.as_ref(),
        state.feature_evaluator.as_ref(),
        org_id,
    )
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    let evaluated = features
        .into_iter()
        .find(|f| f.slug == slug)
        .ok_or(AppError::FeatureUnknown)?;
    Ok(Json(evaluated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/features/orgs/{org_id}/{slug}",
    tag = "features",
    security(("bearer" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organisation ID"),
        ("slug" = String, Path, description = "Feature slug"),
    ),
    responses(
        (status = 204, description = "Override cleared"),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied or not self-service", body = crate::errors::ErrorBody),
        (status = 404, description = "Organisation or feature not found", body = crate::errors::ErrorBody),
    ),
)]
pub async fn delete_org_feature(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<FeaturesManage>,
    axum::extract::Path((org_id, slug)): axum::extract::Path<(Uuid, String)>,
) -> Result<StatusCode, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    clear_org_feature(
        state.features.as_ref(),
        state.feature_evaluator.as_ref(),
        state.audit_recorder.as_ref(),
        ClearOrgFeatureInput {
            organisation_id: org_id,
            slug,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
            actor_is_operator: caller.permissions.is_operator_over_tenants(),
        },
    )
    .await
    .map_err(|e| match e {
        ClearOrgFeatureError::UnknownSlug => AppError::FeatureUnknown,
        ClearOrgFeatureError::NotSelfService => AppError::FeatureNotSelfService,
        ClearOrgFeatureError::Other(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}
