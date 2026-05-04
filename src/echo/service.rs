use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::extractors::Caller;

/// Response body returned by both GET and POST /api/v1/echo.
#[derive(Debug, Serialize, ToSchema)]
pub struct EchoResponse {
    /// HTTP method of the incoming request ("GET" or "POST").
    pub method: String,
    /// The JSON payload sent in the request body, or `null` for GET.
    #[schema(value_type = Object, nullable = true)]
    pub payload: Option<serde_json::Value>,
    /// Organisation the caller belongs to.
    pub org_id: Uuid,
    /// API key ID when authenticated via API key; `null` for JWT callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<Uuid>,
    /// The principal user ID (human user or service account user).
    pub principal_user_id: Uuid,
    /// Server-side timestamp at which the request was received (RFC 3339).
    pub received_at: DateTime<Utc>,
}

/// Build an `EchoResponse` from request context. Pure function — no I/O.
pub fn build_echo(method: &str, body: Option<serde_json::Value>, caller: &Caller) -> EchoResponse {
    let (org_id, key_id, principal_user_id) = match caller {
        Caller::User {
            user_id, org_id, ..
        } => (*org_id, None, *user_id),
        Caller::ApiKey {
            key_id,
            sa_user_id,
            org_id,
        } => (*org_id, Some(*key_id), *sa_user_id),
    };

    EchoResponse {
        method: method.to_string(),
        payload: body,
        org_id,
        key_id,
        principal_user_id,
        received_at: Utc::now(),
    }
}
