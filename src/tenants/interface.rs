use std::collections::HashMap;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::app_state::AppState;
use crate::auth::extractors::{AuthedCaller, Perm, TenantsCreate};
use crate::errors::AppError;
use crate::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};
use crate::tenants::service::list_my_organisations::{
    list_my_organisations, ListError as ListMyOrgsError, ListMyOrganisationsInput,
    OrganisationSummaryDto,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/organisations", post(post_create_organisation))
        .route("/me/organisations", get(get_list_my_organisations))
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
    _perm: Perm<TenantsCreate>,
    Json(req): Json<CreateOrganisationRequest>,
) -> Result<(StatusCode, Json<OrganisationBody>), AppError> {
    // Permission check now happens in the extractor — no inline check here.
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
        CreateOrganisationError::InvalidName => {
            let mut errs = std::collections::HashMap::new();
            errs.insert(
                "name".into(),
                vec!["invalid: must be non-empty and ≤ 120 chars".into()],
            );
            AppError::Validation { errors: errs }
        }
        CreateOrganisationError::InvalidBusiness => {
            let mut errs = std::collections::HashMap::new();
            errs.insert(
                "business".into(),
                vec!["invalid: must be non-empty and ≤ 120 chars".into()],
            );
            AppError::Validation { errors: errs }
        }
        CreateOrganisationError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        CreateOrganisationError::Internal(err) => AppError::Internal(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PagedOrganisations {
    pub items: Vec<OrganisationBody>,
    pub next_cursor: Option<String>,
}

async fn get_list_my_organisations(
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Result<Json<PagedOrganisations>, AppError> {
    let out = list_my_organisations(
        &state,
        caller.claims.sub,
        ListMyOrganisationsInput {
            after: q.after,
            limit: q.limit.unwrap_or(50),
        },
    )
    .await
    .map_err(map_list_my_orgs_error)?;

    Ok(Json(PagedOrganisations {
        items: out
            .items
            .into_iter()
            .map(|o: OrganisationSummaryDto| OrganisationBody {
                id: o.id,
                name: o.name,
                business: o.business,
                role_codes: o.role_codes,
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}

fn map_list_my_orgs_error(e: ListMyOrgsError) -> AppError {
    match e {
        ListMyOrgsError::InvalidCursor => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("after".into(), vec!["invalid_cursor".into()]);
            AppError::Validation { errors: errs }
        }
        ListMyOrgsError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
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
