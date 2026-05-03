use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::auth::extractors::{
    AuthedCaller, Perm, RequireHumanCaller, ServiceAccountsManage, ServiceAccountsRead,
    TenantsMembersAdd, UsersRead,
};
use crate::errors::AppError;
use crate::security::model::UserMembership;
use crate::security::service::change_password::{
    change_password, ChangePasswordError, ChangePasswordInput,
};
use crate::security::service::create_api_key::{
    create_api_key, CreateApiKeyError, CreateApiKeyInput,
};
use crate::security::service::create_service_account::{
    create_service_account, CreateServiceAccountError, CreateServiceAccountInput,
};
use crate::security::service::delete_service_account::{
    delete_service_account, DeleteServiceAccountError, DeleteServiceAccountInput,
};
use crate::security::service::list_api_keys::{list_api_keys, ListApiKeysError, ListApiKeysInput};
use crate::security::service::list_service_accounts::{
    list_service_accounts, ListServiceAccountsInput,
};
use crate::security::service::list_users::{list_users, ListUsersError, ListUsersInput};
use crate::security::service::login::{login, LoginError, LoginInput};
use crate::security::service::logout::{logout, LogoutError};
use crate::security::service::password_reset_confirm::{
    password_reset_confirm, PasswordResetConfirmError, PasswordResetConfirmInput,
};
use crate::security::service::password_reset_request::{
    password_reset_request, PasswordResetRequestError, PasswordResetRequestInput,
};
use crate::security::service::register_user::{
    register_user, RegisterUserError, RegisterUserInput,
};
use crate::security::service::revoke_api_key::{
    revoke_api_key, RevokeApiKeyError, RevokeApiKeyInput,
};
use crate::security::service::rotate_api_key::{
    rotate_api_key, RotateApiKeyError, RotateApiKeyInput,
};
use crate::security::service::switch_org::{switch_org, SwitchOrgError, SwitchOrgInput};

// ── Routers ──────────────────────────────────────────────────────────────────

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/login", post(post_login))
        .route("/password-reset-request", post(post_password_reset_request))
        .route("/password-reset-confirm", post(post_password_reset_confirm))
}

pub fn protected_router() -> Router<AppState> {
    use axum::routing::{delete, get};
    Router::new()
        .route("/register", post(post_register))
        .route("/logout", post(post_logout))
        .route("/change-password", post(post_change_password))
        .route("/switch-org", post(post_switch_org))
        .route(
            "/service-accounts",
            post(post_create_service_account).get(get_list_service_accounts),
        )
        .route(
            "/service-accounts/:sa_id",
            get(get_service_account).delete(delete_service_account_handler),
        )
        .route(
            "/service-accounts/:sa_id/api-keys",
            post(post_create_api_key).get(get_list_api_keys),
        )
        .route(
            "/service-accounts/:sa_id/api-keys/:key_id",
            delete(delete_api_key_handler),
        )
        .route(
            "/service-accounts/:sa_id/api-keys/:key_id/rotate",
            post(post_rotate_api_key),
        )
}

// ── Request / Response bodies ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub org_id: Uuid,
    pub role_code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    pub user_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username_or_email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MembershipDto {
    pub org_id: Uuid,
    pub org_name: String,
    pub role_codes: Vec<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: Uuid,
    pub active_org_id: Uuid,
    pub memberships: Vec<MembershipDto>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SwitchOrgRequest {
    pub org_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordResetRequestBody {
    pub email: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordResetConfirmBody {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListUsersQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub org_id: Option<Uuid>,
    pub q: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserSummaryDto {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub memberships: Vec<MembershipDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListUsersResponse {
    pub items: Vec<UserSummaryDto>,
    pub next_cursor: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/v1/security/register",
    tag = "security",
    request_body = RegisterRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "User registered", body = RegisterResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 409, description = "Duplicate username or email", body = ErrorBody),
    ),
)]
pub async fn post_register(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<TenantsMembersAdd>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    let out = register_user(
        &state,
        caller.claims.sub,
        caller.claims.org,
        RegisterUserInput {
            username: req.username,
            email: req.email,
            password: req.password,
            target_org_id: req.org_id,
            role_code: req.role_code,
        },
    )
    .await
    .map_err(|e| match e {
        RegisterUserError::DuplicateUsername => AppError::Conflict {
            reason: "username already taken".into(),
        },
        RegisterUserError::DuplicateEmail => AppError::Conflict {
            reason: "email already registered".into(),
        },
        RegisterUserError::InvalidUsername => field_error("username", "invalid"),
        RegisterUserError::InvalidEmail => field_error("email", "invalid"),
        RegisterUserError::PasswordTooShort => field_error("password", "too_short"),
        RegisterUserError::PasswordTooLong => field_error("password", "too_long"),
        RegisterUserError::OrgNotFound => AppError::NotFound {
            resource: "organisation".into(),
        },
        RegisterUserError::UnknownRoleCode => field_error("role_code", "unknown_role_code"),
        RegisterUserError::Internal(e) => AppError::Internal(e),
    })?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            user_id: out.user_id,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/login",
    tag = "security",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials", body = ErrorBody),
    ),
)]
pub async fn post_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let out = login(
        &state,
        LoginInput {
            username_or_email: req.username_or_email,
            password: req.password,
        },
    )
    .await
    .map_err(|e| match e {
        LoginError::InvalidCredentials => AppError::InvalidCredentials,
        LoginError::NoOrganisation => AppError::UserNoOrganisation,
        LoginError::Internal(e) => AppError::Internal(e),
    })?;

    Ok(Json(LoginResponse {
        token: out.token,
        user_id: out.user_id,
        active_org_id: out.active_org_id,
        memberships: out
            .memberships
            .into_iter()
            .map(|m: UserMembership| MembershipDto {
                org_id: m.org_id,
                org_name: m.org_name,
                role_codes: m.role_codes,
                joined_at: m.joined_at,
            })
            .collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/logout",
    tag = "security",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Logged out"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
    ),
)]
pub async fn post_logout(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
) -> Result<StatusCode, AppError> {
    let token_expires_at =
        chrono::DateTime::from_timestamp(caller.claims.exp, 0).unwrap_or_else(chrono::Utc::now);
    logout(
        &state,
        caller.claims.sub,
        caller.claims.org,
        caller.claims.jti,
        token_expires_at,
    )
    .await
    .map_err(|LogoutError::Internal(e)| AppError::Internal(e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/change-password",
    tag = "security",
    request_body = ChangePasswordRequest,
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated or wrong current password", body = ErrorBody),
    ),
)]
pub async fn post_change_password(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    change_password(
        &state,
        caller.claims.sub,
        ChangePasswordInput {
            current_password: req.current_password,
            new_password: req.new_password,
        },
    )
    .await
    .map_err(|e| match e {
        ChangePasswordError::WrongCurrentPassword => AppError::InvalidCredentials,
        ChangePasswordError::PasswordTooShort => field_error("new_password", "too_short"),
        ChangePasswordError::UserNotFound => AppError::NotFound {
            resource: "user".into(),
        },
        ChangePasswordError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/switch-org",
    tag = "security",
    request_body = SwitchOrgRequest,
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Org switched — new JWT", body = TokenResponse),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Not a member of target org", body = ErrorBody),
    ),
)]
pub async fn post_switch_org(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    Json(req): Json<SwitchOrgRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let out = switch_org(
        &state,
        caller.claims.sub,
        caller.claims.org,
        SwitchOrgInput {
            target_org_id: req.org_id,
        },
    )
    .await
    .map_err(|e| match e {
        SwitchOrgError::NotMember => AppError::PermissionDenied {
            code: "not_member".into(),
        },
        SwitchOrgError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(Json(TokenResponse { token: out.token }))
}

#[utoipa::path(
    post,
    path = "/api/v1/security/password-reset-request",
    tag = "security",
    request_body = PasswordResetRequestBody,
    responses(
        (status = 204, description = "Reset email dispatched (always)"),
    ),
)]
pub async fn post_password_reset_request(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetRequestBody>,
) -> Result<StatusCode, AppError> {
    let base_url =
        std::env::var("APP_BASE_URL").unwrap_or_else(|_| "https://example.com".to_string());
    password_reset_request(
        &state,
        PasswordResetRequestInput {
            email: req.email,
            base_url,
        },
    )
    .await
    .map_err(|PasswordResetRequestError::Internal(e)| AppError::Internal(e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/password-reset-confirm",
    tag = "security",
    request_body = PasswordResetConfirmBody,
    responses(
        (status = 204, description = "Password reset"),
        (status = 400, description = "Token invalid or expired", body = ErrorBody),
    ),
)]
pub async fn post_password_reset_confirm(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetConfirmBody>,
) -> Result<StatusCode, AppError> {
    password_reset_confirm(
        &state,
        PasswordResetConfirmInput {
            raw_token: req.token,
            new_password: req.new_password,
        },
    )
    .await
    .map_err(|e| match e {
        PasswordResetConfirmError::InvalidToken => AppError::InvalidCredentials,
        PasswordResetConfirmError::PasswordTooShort => field_error("new_password", "too_short"),
        PasswordResetConfirmError::Internal(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/users",
    tag = "security",
    params(ListUsersQuery),
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Paginated user list", body = ListUsersResponse),
        (status = 400, description = "Invalid cursor or limit", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
    ),
)]
pub async fn get_list_users(
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<UsersRead>,
    Query(q): Query<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, AppError> {
    let is_operator = caller.permissions.is_operator_over_users();
    let caller_org_id = if is_operator {
        None
    } else {
        Some(caller.claims.org)
    };

    let limit = q.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(field_error("limit", "invalid_limit"));
    }

    let out = list_users(
        &state,
        caller.claims.sub,
        is_operator,
        caller_org_id,
        ListUsersInput {
            org_id: q.org_id,
            q: q.q,
            after: q.after,
            limit,
        },
    )
    .await
    .map_err(|e| match e {
        ListUsersError::InvalidCursor => field_error("after", "invalid_cursor"),
        ListUsersError::Repo(e) => AppError::Internal(e.into()),
    })?;

    let event = AuditEvent::users_list(caller.claims.sub, caller.claims.org);
    if let Err(e) = state.audit_recorder.record(event).await {
        tracing::warn!(error = %e, "audit record failed for users.list");
    }

    Ok(Json(ListUsersResponse {
        items: out
            .items
            .into_iter()
            .map(|u| UserSummaryDto {
                id: u.id,
                username: u.username,
                email: u.email,
                created_at: u.created_at,
                memberships: u
                    .memberships
                    .into_iter()
                    .map(|m| MembershipDto {
                        org_id: m.org_id,
                        org_name: m.org_name,
                        role_codes: m.role_codes,
                        joined_at: m.joined_at,
                    })
                    .collect(),
            })
            .collect(),
        next_cursor: out.next_cursor,
    }))
}

// ── Service-account / API-key DTOs ────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateServiceAccountRequest {
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceAccountResponse {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<crate::security::model::ServiceAccount> for ServiceAccountResponse {
    fn from(sa: crate::security::model::ServiceAccount) -> Self {
        Self {
            user_id: sa.user_id,
            organisation_id: sa.organisation_id,
            name: sa.name,
            description: sa.description,
            created_at: sa.created_at,
            last_used_at: sa.last_used_at,
        }
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListServiceAccountsQuery {
    pub organisation_id: Uuid,
    pub limit: Option<u32>,
    pub after: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListServiceAccountsResponse {
    pub items: Vec<ServiceAccountResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    pub name: String,
    /// `null` = inherit all of the service account's permissions.
    /// Empty array is rejected.
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub prefix: String,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<crate::security::model::ApiKey> for ApiKeyResponse {
    fn from(k: crate::security::model::ApiKey) -> Self {
        Self {
            id: k.id,
            prefix: k.prefix,
            name: k.name,
            scopes: k.scopes,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
            revoked_at: k.revoked_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateApiKeyResponse {
    pub key: ApiKeyResponse,
    /// Plaintext token. Returned exactly once at creation time; the server keeps
    /// only the argon2 hash. Show it to the operator immediately.
    pub plaintext: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListApiKeysResponse {
    pub items: Vec<ApiKeyResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RotateApiKeyRequest {
    pub name: Option<String>,
    /// `null` = keep existing scopes; `Some(Some(scopes))` = override; `Some(None)` is
    /// not representable in JSON, so the wire-level meaning of an absent field is
    /// "inherit existing".
    pub scopes: Option<Vec<String>>,
}

// ── Service-account / API-key handlers ────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/v1/security/service-accounts",
    tag = "service-accounts",
    request_body = CreateServiceAccountRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "Service account created", body = ServiceAccountResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied or requires user credentials", body = ErrorBody),
        (status = 409, description = "Duplicate name", body = ErrorBody),
    ),
)]
pub async fn post_create_service_account(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsManage>,
    Json(req): Json<CreateServiceAccountRequest>,
) -> Result<(StatusCode, Json<ServiceAccountResponse>), AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != req.organisation_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }

    let sa = create_service_account(
        &state,
        CreateServiceAccountInput {
            organisation_id: req.organisation_id,
            name: req.name,
            description: req.description,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        CreateServiceAccountError::DuplicateName => AppError::Conflict {
            reason: "service-account name already used in this organisation".into(),
        },
        CreateServiceAccountError::Other(e) => AppError::Internal(e),
    })?;

    Ok((StatusCode::CREATED, Json(sa.into())))
}

#[utoipa::path(
    get,
    path = "/api/v1/security/service-accounts",
    tag = "service-accounts",
    params(ListServiceAccountsQuery),
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Paginated list", body = ListServiceAccountsResponse),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
    ),
)]
pub async fn get_list_service_accounts(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsRead>,
    Query(q): Query<ListServiceAccountsQuery>,
) -> Result<Json<ListServiceAccountsResponse>, AppError> {
    if !caller.permissions.is_operator_over_tenants() && caller.claims.org != q.organisation_id {
        return Err(AppError::NotFound {
            resource: "organisation".into(),
        });
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let after = match q.after.as_deref() {
        None => None,
        Some(s) => Some(decode_sa_cursor(s)?),
    };

    let out = list_service_accounts(
        &state,
        ListServiceAccountsInput {
            organisation_id: q.organisation_id,
            limit,
            after,
        },
    )
    .await
    .map_err(AppError::Internal)?;

    Ok(Json(ListServiceAccountsResponse {
        items: out.items.into_iter().map(Into::into).collect(),
        next_cursor: out.next_cursor.map(encode_sa_cursor),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/security/service-accounts/{sa_id}",
    tag = "service-accounts",
    params(("sa_id" = Uuid, Path, description = "Service account user_id")),
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Service account", body = ServiceAccountResponse),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
)]
pub async fn get_service_account(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsRead>,
    axum::extract::Path(sa_id): axum::extract::Path<Uuid>,
) -> Result<Json<ServiceAccountResponse>, AppError> {
    let org = caller.claims.org;
    let sa = state
        .service_accounts
        .find(org, sa_id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound {
            resource: "service_account".into(),
        })?;
    Ok(Json(sa.into()))
}

#[utoipa::path(
    delete,
    path = "/api/v1/security/service-accounts/{sa_id}",
    tag = "service-accounts",
    params(("sa_id" = Uuid, Path, description = "Service account user_id")),
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
)]
pub async fn delete_service_account_handler(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsManage>,
    axum::extract::Path(sa_id): axum::extract::Path<Uuid>,
) -> Result<StatusCode, AppError> {
    delete_service_account(
        &state,
        DeleteServiceAccountInput {
            organisation_id: caller.claims.org,
            sa_user_id: sa_id,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        DeleteServiceAccountError::NotFound => AppError::NotFound {
            resource: "service_account".into(),
        },
        DeleteServiceAccountError::Other(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/service-accounts/{sa_id}/api-keys",
    tag = "service-accounts",
    params(("sa_id" = Uuid, Path, description = "Service account user_id")),
    request_body = CreateApiKeyRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "Plaintext returned ONCE", body = CreateApiKeyResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Service account not found", body = ErrorBody),
    ),
)]
pub async fn post_create_api_key(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsManage>,
    axum::extract::Path(sa_id): axum::extract::Path<Uuid>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), AppError> {
    let mat = create_api_key(
        &state,
        CreateApiKeyInput {
            organisation_id: caller.claims.org,
            sa_user_id: sa_id,
            name: req.name,
            scopes: req.scopes,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        CreateApiKeyError::NotFound => AppError::NotFound {
            resource: "service_account".into(),
        },
        CreateApiKeyError::EmptyScopes => field_error("scopes", "empty_scopes"),
        CreateApiKeyError::PrefixCollision => AppError::Internal(anyhow::anyhow!(
            "could not allocate unique key prefix; please retry"
        )),
        CreateApiKeyError::Other(e) => AppError::Internal(e),
    })?;
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            key: mat.key.into(),
            plaintext: mat.plaintext,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/security/service-accounts/{sa_id}/api-keys",
    tag = "service-accounts",
    params(("sa_id" = Uuid, Path, description = "Service account user_id")),
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Keys (metadata only)", body = ListApiKeysResponse),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Service account not found", body = ErrorBody),
    ),
)]
pub async fn get_list_api_keys(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsRead>,
    axum::extract::Path(sa_id): axum::extract::Path<Uuid>,
) -> Result<Json<ListApiKeysResponse>, AppError> {
    let keys = list_api_keys(
        &state,
        ListApiKeysInput {
            organisation_id: caller.claims.org,
            sa_user_id: sa_id,
        },
    )
    .await
    .map_err(|e| match e {
        ListApiKeysError::NotFound => AppError::NotFound {
            resource: "service_account".into(),
        },
        ListApiKeysError::Other(e) => AppError::Internal(e),
    })?;
    Ok(Json(ListApiKeysResponse {
        items: keys.into_iter().map(Into::into).collect(),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}",
    tag = "service-accounts",
    params(
        ("sa_id" = Uuid, Path, description = "Service account user_id"),
        ("key_id" = Uuid, Path, description = "API key id"),
    ),
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Not found / already revoked", body = ErrorBody),
    ),
)]
pub async fn delete_api_key_handler(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsManage>,
    axum::extract::Path((sa_id, key_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    revoke_api_key(
        &state,
        RevokeApiKeyInput {
            organisation_id: caller.claims.org,
            sa_user_id: sa_id,
            key_id,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        RevokeApiKeyError::NotFound | RevokeApiKeyError::KeyNotFound => AppError::NotFound {
            resource: "api_key".into(),
        },
        RevokeApiKeyError::Other(e) => AppError::Internal(e),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}/rotate",
    tag = "service-accounts",
    params(
        ("sa_id" = Uuid, Path, description = "Service account user_id"),
        ("key_id" = Uuid, Path, description = "API key id"),
    ),
    request_body = RotateApiKeyRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, description = "New key minted, old revoked", body = CreateApiKeyResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthenticated", body = ErrorBody),
        (status = 403, description = "Permission denied", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
)]
pub async fn post_rotate_api_key(
    _gate: RequireHumanCaller,
    State(state): State<AppState>,
    caller: AuthedCaller,
    _perm: Perm<ServiceAccountsManage>,
    axum::extract::Path((sa_id, key_id)): axum::extract::Path<(Uuid, Uuid)>,
    Json(req): Json<RotateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), AppError> {
    let scopes_override = req.scopes.map(Some); // Some(Some(...)) means "set to this list"
    let mat = rotate_api_key(
        &state,
        RotateApiKeyInput {
            organisation_id: caller.claims.org,
            sa_user_id: sa_id,
            old_key_id: key_id,
            name: req.name,
            scopes: scopes_override,
            actor_user_id: caller.claims.sub,
            actor_org_id: caller.claims.org,
        },
    )
    .await
    .map_err(|e| match e {
        RotateApiKeyError::NotFound | RotateApiKeyError::KeyNotFound => AppError::NotFound {
            resource: "api_key".into(),
        },
        RotateApiKeyError::EmptyScopes => field_error("scopes", "empty_scopes"),
        RotateApiKeyError::PrefixCollision => AppError::Internal(anyhow::anyhow!(
            "could not allocate unique key prefix; please retry"
        )),
        RotateApiKeyError::Other(e) => AppError::Internal(e),
    })?;
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            key: mat.key.into(),
            plaintext: mat.plaintext,
        }),
    ))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn field_error(field: &str, code: &str) -> AppError {
    let mut errs = HashMap::new();
    errs.insert(field.to_string(), vec![code.to_string()]);
    AppError::Validation { errors: errs }
}

fn encode_sa_cursor(c: (chrono::DateTime<chrono::Utc>, Uuid)) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let raw = format!("{}|{}", c.0.to_rfc3339(), c.1);
    URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

fn decode_sa_cursor(s: &str) -> Result<(chrono::DateTime<chrono::Utc>, Uuid), AppError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| field_error("after", "invalid_cursor"))?;
    let raw = std::str::from_utf8(&bytes).map_err(|_| field_error("after", "invalid_cursor"))?;
    let mut parts = raw.splitn(2, '|');
    let ts_str = parts
        .next()
        .ok_or_else(|| field_error("after", "invalid_cursor"))?;
    let id_str = parts
        .next()
        .ok_or_else(|| field_error("after", "invalid_cursor"))?;
    let ts = chrono::DateTime::parse_from_rfc3339(ts_str)
        .map_err(|_| field_error("after", "invalid_cursor"))?
        .with_timezone(&chrono::Utc);
    let id = Uuid::parse_str(id_str).map_err(|_| field_error("after", "invalid_cursor"))?;
    Ok((ts, id))
}
