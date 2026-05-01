use std::collections::HashMap;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::app_state::AppState;
use crate::auth::extractors::{
    AuthedCaller, ChannelsManage, Perm, TenantsCreate, TenantsMembersAdd, TenantsMembersList,
    TenantsMembersRemove, TenantsRolesAssign,
};
use crate::errors::AppError;
use crate::tenants::model::{ChannelType, InboundChannel};
use crate::tenants::service::add_user_to_organisation::{
    add_user_to_organisation, AddUserToOrganisationError, AddUserToOrganisationInput,
};
use crate::tenants::service::assign_role::{assign_role, AssignRoleError, AssignRoleInput};
use crate::tenants::service::create_inbound_channel::{
    create_inbound_channel, CreateChannelError, CreateChannelInput,
};
use crate::tenants::service::create_organisation::{
    create_organisation, CreateOrganisationError, CreateOrganisationInput,
};
use crate::tenants::service::delete_inbound_channel::{delete_inbound_channel, DeleteChannelError};
use crate::tenants::service::get_inbound_channel::{get_inbound_channel, GetChannelError};
use crate::tenants::service::list_inbound_channels::{
    list_inbound_channels, ListChannelsError, ListChannelsInput,
};
use crate::tenants::service::list_my_organisations::{
    list_my_organisations, ListError as ListMyOrgsError, ListMyOrganisationsInput,
    OrganisationSummaryDto,
};
use crate::tenants::service::list_organisation_members::{
    list_organisation_members, ListMembersError, ListMembersInput,
};
use crate::tenants::service::remove_user_from_organisation::{
    remove_user_from_organisation, RemoveUserFromOrganisationError, RemoveUserFromOrganisationInput,
};
use crate::tenants::service::update_inbound_channel::{
    update_inbound_channel, UpdateChannelError, UpdateChannelInput,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/organisations", post(post_create_organisation))
        .route("/me/organisations", get(get_list_my_organisations))
        .route("/organisations/:id/members", get(get_list_members))
        .route("/organisations/:id/memberships", post(post_assign_role))
        .route(
            "/add-user-to-organisation",
            post(post_add_user_to_organisation),
        )
        .route(
            "/remove-user-from-organisation",
            post(post_remove_user_from_organisation),
        )
        .route(
            "/organisations/:org_id/channels",
            post(post_create_channel).get(get_list_channels),
        )
        .route(
            "/organisations/:org_id/channels/:channel_id",
            get(get_channel)
                .put(put_update_channel)
                .delete(delete_channel),
        )
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddUserToOrganisationRequest {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub role_code: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/tenants/add-user-to-organisation",
    tag = "tenants",
    request_body = AddUserToOrganisationRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "User added to organisation"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Organisation or user not found", body = ErrorBody),
    ),
)]
pub async fn post_add_user_to_organisation(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersAdd>,
    Json(req): Json<AddUserToOrganisationRequest>,
) -> Result<StatusCode, AppError> {
    add_user_to_organisation(
        &state,
        caller.claims.sub,
        caller.claims.org,
        AddUserToOrganisationInput {
            user_id: req.user_id,
            org_id: req.org_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(|e| match e {
        AddUserToOrganisationError::NotFound => AppError::NotFound {
            resource: "organisation or user".into(),
        },
        AddUserToOrganisationError::UnknownRoleCode => {
            let mut errs = std::collections::HashMap::new();
            errs.insert("role_code".into(), vec!["unknown_role_code".into()]);
            AppError::Validation { errors: errs }
        }
        AddUserToOrganisationError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
        AddUserToOrganisationError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RemoveUserFromOrganisationRequest {
    pub user_id: Uuid,
    pub org_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/v1/tenants/remove-user-from-organisation",
    tag = "tenants",
    request_body = RemoveUserFromOrganisationRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "User removed"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "User is not a member", body = ErrorBody),
        (status = 409, description = "Cannot remove last owner", body = ErrorBody),
    ),
)]
pub async fn post_remove_user_from_organisation(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersRemove>,
    Json(req): Json<RemoveUserFromOrganisationRequest>,
) -> Result<StatusCode, AppError> {
    remove_user_from_organisation(
        &state,
        caller.claims.sub,
        caller.claims.org,
        RemoveUserFromOrganisationInput {
            user_id: req.user_id,
            org_id: req.org_id,
        },
    )
    .await
    .map_err(|e| match e {
        RemoveUserFromOrganisationError::NotMember => AppError::NotFound {
            resource: "membership".into(),
        },
        RemoveUserFromOrganisationError::LastOwner => AppError::Conflict {
            reason: "cannot remove the last owner".into(),
        },
        RemoveUserFromOrganisationError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
        RemoveUserFromOrganisationError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
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

// ── Channel DTOs ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateChannelRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateChannelRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub is_active: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChannelBody {
    pub id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub api_key: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<InboundChannel> for ChannelBody {
    fn from(ch: InboundChannel) -> Self {
        Self {
            id: ch.id,
            organisation_id: ch.organisation_id,
            name: ch.name,
            description: ch.description,
            channel_type: ch.channel_type,
            api_key: ch.api_key,
            is_active: ch.is_active,
            created_at: ch.created_at,
            updated_at: ch.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PagedChannels {
    pub items: Vec<ChannelBody>,
    pub next_cursor: Option<String>,
}

// ── Channel error mappers ─────────────────────────────────────────────────────

fn map_create_channel_error(e: CreateChannelError) -> AppError {
    match e {
        CreateChannelError::DuplicateName => AppError::Conflict {
            reason: "channel name already taken in this organisation".into(),
        },
        CreateChannelError::InvalidName | CreateChannelError::InvalidDescription => {
            AppError::Validation {
                errors: HashMap::from([("name".into(), vec!["invalid".into()])]),
            }
        }
        CreateChannelError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
    }
}

fn map_update_channel_error(e: UpdateChannelError) -> AppError {
    match e {
        UpdateChannelError::NotFound => AppError::NotFound {
            resource: "channel".into(),
        },
        UpdateChannelError::DuplicateName => AppError::Conflict {
            reason: "channel name already taken".into(),
        },
        UpdateChannelError::InvalidName | UpdateChannelError::InvalidDescription => {
            AppError::Validation {
                errors: HashMap::from([("name".into(), vec!["invalid".into()])]),
            }
        }
        UpdateChannelError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
    }
}

// ── Channel handlers ──────────────────────────────────────────────────────────

pub async fn post_create_channel(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ChannelsManage>,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    Json(req): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelBody>), AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;
    let ch = create_inbound_channel(
        &state,
        caller.claims.sub,
        caller.claims.org,
        CreateChannelInput {
            organisation_id: org_id,
            name: req.name,
            description: req.description,
            channel_type: req.channel_type,
            is_active: req.is_active,
        },
    )
    .await
    .map_err(map_create_channel_error)?;
    Ok((StatusCode::CREATED, Json(ChannelBody::from(ch))))
}

pub async fn get_list_channels(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ChannelsManage>,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<PagedChannels>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    let after = params.get("after").cloned();
    let limit: u32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(200);
    let out = list_inbound_channels(
        &state,
        ListChannelsInput {
            organisation_id: org_id,
            after,
            limit,
        },
    )
    .await
    .map_err(|e| match e {
        ListChannelsError::InvalidCursor => AppError::Validation {
            errors: HashMap::from([("after".into(), vec!["invalid_cursor".into()])]),
        },
        ListChannelsError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
    })?;
    Ok(Json(PagedChannels {
        items: out.items.into_iter().map(ChannelBody::from).collect(),
        next_cursor: out.next_cursor,
    }))
}

pub async fn get_channel(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ChannelsManage>,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<Json<ChannelBody>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    let ch = get_inbound_channel(&state, org_id, channel_id)
        .await
        .map_err(|e| match e {
            GetChannelError::NotFound => AppError::NotFound {
                resource: "channel".into(),
            },
            GetChannelError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
        })?;
    Ok(Json(ChannelBody::from(ch)))
}

pub async fn put_update_channel(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ChannelsManage>,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateChannelRequest>,
) -> Result<Json<ChannelBody>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    req.validate().map_err(|e| AppError::Validation {
        errors: validation_errors_to_map(e),
    })?;
    let ch = update_inbound_channel(
        &state,
        caller.claims.sub,
        caller.claims.org,
        UpdateChannelInput {
            organisation_id: org_id,
            channel_id,
            name: req.name,
            description: req.description,
            channel_type: req.channel_type,
            is_active: req.is_active,
        },
    )
    .await
    .map_err(map_update_channel_error)?;
    Ok(Json(ChannelBody::from(ch)))
}

pub async fn delete_channel(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ChannelsManage>,
    axum::extract::Path((org_id, channel_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != org_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    delete_inbound_channel(
        &state,
        caller.claims.sub,
        caller.claims.org,
        org_id,
        channel_id,
    )
    .await
    .map_err(|e| match e {
        DeleteChannelError::NotFound => AppError::NotFound {
            resource: "channel".into(),
        },
        DeleteChannelError::Repo(e) => AppError::Internal(anyhow::anyhow!(e)),
    })?;
    Ok(StatusCode::NO_CONTENT)
}
