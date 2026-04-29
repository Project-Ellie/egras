use std::collections::HashMap;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::auth::extractors::{AuthedCaller, Perm, TenantsMembersAdd};
use crate::errors::AppError;
use crate::security::model::UserMembership;
use crate::security::service::change_password::{
    change_password, ChangePasswordError, ChangePasswordInput,
};
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
use crate::security::service::switch_org::{switch_org, SwitchOrgError, SwitchOrgInput};

// ── Routers ──────────────────────────────────────────────────────────────────

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/login", post(post_login))
        .route("/password-reset-request", post(post_password_reset_request))
        .route("/password-reset-confirm", post(post_password_reset_confirm))
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/register", post(post_register))
        .route("/logout", post(post_logout))
        .route("/change-password", post(post_change_password))
        .route("/switch-org", post(post_switch_org))
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
    State(state): State<AppState>,
    caller: AuthedCaller,
) -> Result<StatusCode, AppError> {
    logout(
        &state,
        caller.claims.sub,
        caller.claims.org,
        caller.claims.jti,
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn field_error(field: &str, code: &str) -> AppError {
    let mut errs = HashMap::new();
    errs.insert(field.to_string(), vec![code.to_string()]);
    AppError::Validation { errors: errs }
}
