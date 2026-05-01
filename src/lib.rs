pub mod app_state;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
pub mod openapi;
pub mod security;
pub mod tenants;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::app_state::AppState;
use crate::audit::persistence::AuditRepositoryPg;
use crate::audit::service::{ChannelAuditRecorder, ListAuditEventsImpl};
use crate::audit::worker::{AuditWorker, AuditWorkerHandle};
use crate::auth::middleware::{AuthLayer, PermissionLoader, RevocationChecker};
use crate::config::AppConfig;

pub async fn build_app(
    pool: PgPool,
    cfg: AppConfig,
) -> anyhow::Result<(Router, AuditWorkerHandle)> {
    // 1. Audit infra
    let (audit_tx, audit_rx) = mpsc::channel(cfg.audit_channel_capacity);
    let audit_repo: Arc<dyn crate::audit::persistence::AuditRepository> =
        Arc::new(AuditRepositoryPg::new(pool.clone()));
    let audit_handle = AuditWorker::new(
        audit_rx,
        audit_repo.clone(),
        cfg.audit_max_retries,
        cfg.audit_retry_backoff_ms_initial,
    )
    .spawn();

    let audit_recorder: Arc<dyn crate::audit::service::AuditRecorder> =
        Arc::new(ChannelAuditRecorder::new(audit_tx));
    let list_audit_events: Arc<dyn crate::audit::service::ListAuditEvents> =
        Arc::new(ListAuditEventsImpl::new(audit_repo.clone()));

    let organisations: Arc<dyn crate::tenants::persistence::OrganisationRepository> = Arc::new(
        crate::tenants::persistence::OrganisationRepositoryPg::new(pool.clone()),
    );
    let roles: Arc<dyn crate::tenants::persistence::RoleRepository> = Arc::new(
        crate::tenants::persistence::RoleRepositoryPg::new(pool.clone()),
    );
    let users: Arc<dyn crate::security::persistence::UserRepository> = Arc::new(
        crate::security::persistence::UserRepositoryPg::new(pool.clone()),
    );
    let tokens: Arc<dyn crate::security::persistence::TokenRepository> = Arc::new(
        crate::security::persistence::TokenRepositoryPg::new(pool.clone()),
    );

    let state = AppState {
        pool: pool.clone(),
        audit_recorder,
        list_audit_events,
        organisations,
        roles,
        users,
        tokens,
        jwt_config: crate::auth::jwt::JwtConfig {
            secret: cfg.jwt_secret.clone(),
            issuer: cfg.jwt_issuer.clone(),
            ttl_secs: cfg.jwt_ttl_secs,
        },
        password_reset_ttl_secs: cfg.password_reset_ttl_secs,
    };

    // 2. Public routes (no auth)
    let public = Router::<AppState>::new()
        .route("/health", get(health))
        .route(
            "/ready",
            get({
                let pool = pool.clone();
                move || ready(pool.clone())
            }),
        )
        .nest(
            "/api/v1/security",
            crate::security::interface::public_router(),
        )
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/api-docs/openapi.json", crate::openapi::ApiDoc::openapi()),
        );

    // 3. Protected routes
    let auth_layer = AuthLayer::new(
        cfg.jwt_secret.clone(),
        cfg.jwt_issuer.clone(),
        PermissionLoader::pg(pool.clone()),
        RevocationChecker::pg(pool.clone()),
    );
    let protected: Router<AppState> = Router::new()
        .nest("/api/v1/tenants", crate::tenants::interface::router())
        .nest(
            "/api/v1/security",
            crate::security::interface::protected_router(),
        )
        .route(
            "/api/v1/users",
            axum::routing::get(crate::security::interface::get_list_users),
        )
        .layer(auth_layer);

    // 4. Compose
    let cors = build_cors(&cfg);
    let router = public
        .merge(protected)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    Ok((router, audit_handle))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(
    pool: PgPool,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await
    {
        Ok(_) => Ok(Json(json!({ "status": "ready" }))),
        Err(err) => {
            tracing::warn!(error = %err, "readiness check failed");
            Err((
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "not_ready", "error": err.to_string() })),
            ))
        }
    }
}

fn build_cors(cfg: &AppConfig) -> CorsLayer {
    if cfg.cors_allowed_origins.trim().is_empty() {
        CorsLayer::new()
    } else {
        let origins: Vec<axum::http::HeaderValue> = cfg
            .cors_allowed_origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
            ])
    }
}
