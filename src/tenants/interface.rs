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
use crate::auth::extractors::{
    AuthedCaller, Perm, TenantsCreate, TenantsMembersList, TenantsRolesAssign,
};
use crate::errors::AppError;
use crate::tenants::service::assign_role::{assign_role, AssignRoleError, AssignRoleInput};
use crate::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};
use crate::tenants::service::list_my_organisations::{
    list_my_organisations, ListError as ListMyOrgsError, ListMyOrganisationsInput,
    OrganisationSummaryDto,
};
use crate::tenants::service::list_organisation_members::{
    list_organisation_members, ListMembersError, ListMembersInput,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/organisations", post(post_create_organisation))
        .route("/me/organisations", get(get_list_my_organisations))
        .route("/organisations/:id/members", get(get_list_members))
        .route("/organisations/:id/memberships", post(post_assign_role))
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

#[utoipa::path(
    post,
    path = "/api/v1/tenants/organisations",
    tag = "tenants",
    request_body = CreateOrganisationRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "Organisation created", body = OrganisationBody),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 409, description = "Duplicate organisation name", body = ErrorBody),
    ),
)]
pub async fn post_create_organisation(
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

#[utoipa::path(
    get,
    path = "/api/v1/tenants/me/organisations",
    tag = "tenants",
    security(("bearer" = [])),
    params(
        ("after" = Option<String>, Query, description = "Cursor for pagination"),
        ("limit" = Option<u32>, Query, description = "Maximum items to return (default 50)"),
    ),
    responses(
        (status = 200, description = "Paginated list of the caller's organisations", body = PagedOrganisations),
        (status = 400, description = "Invalid cursor", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
    ),
)]
pub async fn get_list_my_organisations(
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

#[derive(Debug, Serialize, ToSchema)]
pub struct MemberBody {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PagedMembers {
    pub items: Vec<MemberBody>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/tenants/organisations/{id}/members",
    tag = "tenants",
    security(("bearer" = [])),
    params(
        ("id" = Uuid, Path, description = "Organisation ID"),
        ("after" = Option<String>, Query, description = "Cursor for pagination"),
        ("limit" = Option<u32>, Query, description = "Maximum items to return (default 50)"),
    ),
    responses(
        (status = 200, description = "Paginated list of organisation members", body = PagedMembers),
        (status = 400, description = "Invalid cursor", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation not found", body = ErrorBody),
    ),
)]
pub async fn get_list_members(
    _perm: Perm<TenantsMembersList>,
    State(state): State<AppState>,
    caller: AuthedCaller,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Result<Json<PagedMembers>, AppError> {
    let is_operator = caller.permissions.is_operator_over_tenants();
    let out = list_organisation_members(
        &state,
        caller.claims.sub,
        is_operator,
        ListMembersInput {
            organisation_id: id,
            after: q.after,
            limit: q.limit.unwrap_or(50),
        },
    )
    .await
    .map_err(|e| match e {
        ListMembersError::NotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        ListMembersError::InvalidCursor => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("after".into(), vec!["invalid_cursor".into()]);
            AppError::Validation { errors: errs }
        }
        ListMembersError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
    })?;

    Ok(Json(PagedMembers {
        items: out
            .items
            .into_iter()
            .map(|m| MemberBody {
                user_id: m.user_id,
                username: m.username,
                email: m.email,
                role_codes: m.role_codes,
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct AssignRoleRequest {
    pub user_id: Uuid,
    #[validate(length(min = 1, max = 64))]
    pub role_code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignRoleResponseBody {
    pub assigned: bool,
}

#[utoipa::path(
    post,
    path = "/api/v1/tenants/organisations/{id}/memberships",
    tag = "tenants",
    request_body = AssignRoleRequest,
    security(("bearer" = [])),
    params(
        ("id" = Uuid, Path, description = "Organisation ID"),
    ),
    responses(
        (status = 200, description = "Role assigned", body = AssignRoleResponseBody),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation not found", body = ErrorBody),
    ),
)]
pub async fn post_assign_role(
    State(state): State<AppState>,
    _perm: Perm<TenantsRolesAssign>,
    caller: AuthedCaller,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    Json(req): Json<AssignRoleRequest>,
) -> Result<(StatusCode, Json<AssignRoleResponseBody>), AppError> {
    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;

    let out = assign_role(
        &state,
        caller.claims.sub,
        caller.claims.org,
        caller.permissions.is_operator_over_tenants(),
        AssignRoleInput {
            organisation_id: org_id,
            target_user_id: req.user_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(map_assign_role_error)?;

    Ok((
        StatusCode::OK,
        Json(AssignRoleResponseBody {
            assigned: out.was_new,
        }),
    ))
}

fn map_assign_role_error(e: AssignRoleError) -> AppError {
    match e {
        AssignRoleError::NotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        AssignRoleError::UnknownRoleCode => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("role_code".into(), vec!["unknown_role_code".into()]);
            AppError::Validation { errors: errs }
        }
        AssignRoleError::UnknownUser => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("user_id".into(), vec!["not_a_member".into()]);
            AppError::Validation { errors: errs }
        }
        AssignRoleError::Repo(r) => AppError::Internal(anyhow::anyhow!(r)),
        AssignRoleError::Internal(err) => AppError::Internal(err),
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
