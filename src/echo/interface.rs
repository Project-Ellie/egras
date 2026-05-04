use axum::{routing::get, Json, Router};

use crate::app_state::AppState;
use crate::auth::extractors::{Caller, EchoInvoke, Perm};
use crate::echo::service::{build_echo, EchoResponse};
use crate::errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_echo).post(post_echo))
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/v1/echo",
    tag = "echo",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Echo response", body = EchoResponse),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied", body = crate::errors::ErrorBody),
    ),
)]
pub async fn get_echo(
    caller: Caller,
    _perm: Perm<EchoInvoke>,
) -> Result<Json<EchoResponse>, AppError> {
    Ok(Json(build_echo("GET", None, &caller)))
}

#[utoipa::path(
    post,
    path = "/api/v1/echo",
    tag = "echo",
    request_body = serde_json::Value,
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Echo response", body = EchoResponse),
        (status = 401, description = "Unauthenticated", body = crate::errors::ErrorBody),
        (status = 403, description = "Permission denied", body = crate::errors::ErrorBody),
    ),
)]
pub async fn post_echo(
    caller: Caller,
    _perm: Perm<EchoInvoke>,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<EchoResponse>, AppError> {
    let payload = body.map(|Json(v)| v);
    Ok(Json(build_echo("POST", payload, &caller)))
}
