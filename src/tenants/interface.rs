use std::collections::HashMap;

use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::app_state::AppState;
use crate::auth::jwt::Claims;
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;
use crate::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/organisations", post(post_create_organisation))
}

/// Extractor that grabs `(Claims, PermissionSet)` off the request extensions.
/// Runs before the Json body extractor, so permission 403s beat body-parse 400s.
struct AuthedCaller {
    claims: Claims,
    permissions: PermissionSet,
}

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthedCaller {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims =
            parts
                .extensions
                .get::<Claims>()
                .cloned()
                .ok_or_else(|| AppError::Unauthenticated {
                    reason: "no_claims".into(),
                })?;
        let permissions = parts
            .extensions
            .get::<PermissionSet>()
            .cloned()
            .ok_or_else(|| AppError::Unauthenticated {
                reason: "no_permission_set".into(),
            })?;
        Ok(Self {
            claims,
            permissions,
        })
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateOrganisationRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(min = 1, max = 120))]
    pub business: String,
    #[serde(default = "default_true")]
    pub seed_creator_as_owner: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrganisationBody {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
}

async fn post_create_organisation(
    State(state): State<AppState>,
    caller: AuthedCaller,
    Json(req): Json<CreateOrganisationRequest>,
) -> Result<(StatusCode, Json<OrganisationBody>), AppError> {
    // Permission check BEFORE body validation — precedence 401>403>400.
    if !caller.permissions.has("tenants.create") {
        return Err(AppError::PermissionDenied {
            code: "tenants.create".into(),
        });
    }

    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;

    let out = create_organisation(
        &state,
        caller.claims.sub,
        caller.claims.org,
        CreateOrganisationInput {
            name: req.name,
            business: req.business,
            seed_creator_as_owner: req.seed_creator_as_owner,
        },
    )
    .await
    .map_err(map_service_error)?;

    Ok((
        StatusCode::CREATED,
        Json(OrganisationBody {
            id: out.id,
            name: out.name,
            business: out.business,
            role_codes: out.role_codes,
        }),
    ))
}

fn map_service_error(e: CreateOrganisationError) -> AppError {
    match e {
        CreateOrganisationError::DuplicateName => AppError::Conflict {
            reason: "organisation name already exists".into(),
        },
        CreateOrganisationError::InvalidName | CreateOrganisationError::InvalidBusiness => {
            let mut errs = HashMap::new();
            errs.insert("name_or_business".into(), vec![e.to_string()]);
            AppError::Validation { errors: errs }
        }
        CreateOrganisationError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        CreateOrganisationError::Internal(err) => AppError::Internal(err),
    }
}

fn validation_errors_to_map(e: validator::ValidationErrors) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for (field, issues) in e.field_errors() {
        out.insert(
            field.to_string(),
            issues
                .iter()
                .map(|v| v.code.to_string())
                .collect::<Vec<_>>(),
        );
    }
    out
}
