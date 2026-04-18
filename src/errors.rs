use std::collections::HashMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;
use thiserror::Error;

/// Canonical error slugs from spec §8.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorSlug {
    #[serde(rename = "validation.invalid_request")]
    ValidationInvalidRequest,
    #[serde(rename = "auth.unauthenticated")]
    AuthUnauthenticated,
    #[serde(rename = "auth.invalid_credentials")]
    AuthInvalidCredentials,
    #[serde(rename = "permission.denied")]
    PermissionDenied,
    #[serde(rename = "resource.not_found")]
    ResourceNotFound,
    #[serde(rename = "resource.conflict")]
    ResourceConflict,
    #[serde(rename = "user.no_organisation")]
    UserNoOrganisation,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "internal.error")]
    InternalError,
}

impl ErrorSlug {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ValidationInvalidRequest => "validation.invalid_request",
            Self::AuthUnauthenticated => "auth.unauthenticated",
            Self::AuthInvalidCredentials => "auth.invalid_credentials",
            Self::PermissionDenied => "permission.denied",
            Self::ResourceNotFound => "resource.not_found",
            Self::ResourceConflict => "resource.conflict",
            Self::UserNoOrganisation => "user.no_organisation",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal.error",
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation failed")]
    Validation { errors: HashMap<String, Vec<String>> },

    #[error("unauthenticated: {reason}")]
    Unauthenticated { reason: String },

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("permission denied: missing {code}")]
    PermissionDenied { code: String },

    #[error("not found: {resource}")]
    NotFound { resource: String },

    #[error("conflict: {reason}")]
    Conflict { reason: String },

    #[error("user has no organisation")]
    UserNoOrganisation,

    #[error("rate limited")]
    RateLimited,

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn slug(&self) -> ErrorSlug {
        match self {
            Self::Validation { .. } => ErrorSlug::ValidationInvalidRequest,
            Self::Unauthenticated { .. } => ErrorSlug::AuthUnauthenticated,
            Self::InvalidCredentials => ErrorSlug::AuthInvalidCredentials,
            Self::PermissionDenied { .. } => ErrorSlug::PermissionDenied,
            Self::NotFound { .. } => ErrorSlug::ResourceNotFound,
            Self::Conflict { .. } => ErrorSlug::ResourceConflict,
            Self::UserNoOrganisation => ErrorSlug::UserNoOrganisation,
            Self::RateLimited => ErrorSlug::RateLimited,
            Self::Internal(_) => ErrorSlug::InternalError,
        }
    }

    pub fn http_status(&self) -> StatusCode {
        match self {
            Self::Validation { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::PermissionDenied { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::UserNoOrganisation => StatusCode::FORBIDDEN,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.slug() {
            ErrorSlug::ValidationInvalidRequest => "Invalid request",
            ErrorSlug::AuthUnauthenticated => "Unauthenticated",
            ErrorSlug::AuthInvalidCredentials => "Invalid credentials",
            ErrorSlug::PermissionDenied => "Permission denied",
            ErrorSlug::ResourceNotFound => "Not found",
            ErrorSlug::ResourceConflict => "Conflict",
            ErrorSlug::UserNoOrganisation => "User has no organisation",
            ErrorSlug::RateLimited => "Rate limited",
            ErrorSlug::InternalError => "Internal error",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Validation { .. } => "One or more fields failed validation.".to_string(),
            Self::Unauthenticated { reason } => format!("Authentication required ({reason})."),
            Self::InvalidCredentials => "Invalid username or password.".to_string(),
            Self::PermissionDenied { code } => format!("missing permission: {code}"),
            Self::NotFound { resource } => format!("{resource} was not found."),
            Self::Conflict { reason } => reason.clone(),
            Self::UserNoOrganisation => "The user does not belong to any organisation.".to_string(),
            Self::RateLimited => "Too many requests; retry later.".to_string(),
            Self::Internal(_) => "An internal error occurred.".to_string(),
        }
    }
}

const TYPE_PREFIX: &str = "https://egras.dev/errors/";

#[derive(Debug, Serialize)]
struct ProblemJson<'a> {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'a str,
    status: u16,
    detail: String,
    instance: Option<&'a str>,
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a HashMap<String, Vec<String>>>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.http_status();
        let slug = self.slug();
        let title = self.title();
        let detail = self.detail();
        let errors_ref = if let AppError::Validation { errors } = &self { Some(errors) } else { None };

        // Log internal errors with full chain before we drop the error.
        if let AppError::Internal(err) = &self {
            tracing::error!(error.kind = "internal", error.chain = %err, "internal error");
        }

        let body = ProblemJson {
            type_uri: format!("{TYPE_PREFIX}{}", slug.as_str()),
            title,
            status: status.as_u16(),
            detail,
            instance: None,
            request_id: None, // populated by a downstream layer if desired
            errors: errors_ref,
        };

        let mut resp = (status, Json(body)).into_response();
        resp.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        resp
    }
}
