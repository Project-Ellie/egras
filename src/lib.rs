pub mod app_state;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
pub mod jobs;
pub mod openapi;
pub mod outbox;
pub mod pagination;
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
use crate::jobs::persistence::{JobsRepository, JobsRepositoryPg};
use crate::jobs::{JobRunner, JobRunnerConfig, JobRunnerHandle, JobsEnqueuer};
use crate::outbox::persistence::{OutboxRepository, OutboxRepositoryPg};
use crate::outbox::{OutboxAppender, OutboxRelayer, OutboxRelayerConfig, OutboxRelayerHandle};

pub struct AppHandles {
    pub router: Router,
    pub audit: AuditWorkerHandle,
    pub jobs: JobRunnerHandle,
    pub outbox: OutboxRelayerHandle,
}

pub async fn build_app(pool: PgPool, cfg: AppConfig) -> anyhow::Result<AppHandles> {
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
    let inbound_channels: Arc<dyn crate::tenants::persistence::InboundChannelRepository> =
        Arc::new(crate::tenants::persistence::InboundChannelRepositoryPg::new(pool.clone()));
    let service_accounts: Arc<dyn crate::security::persistence::ServiceAccountRepository> =
        Arc::new(crate::security::persistence::ServiceAccountRepositoryPg::new(pool.clone()));
    let api_keys: Arc<dyn crate::security::persistence::ApiKeyRepository> = Arc::new(
        crate::security::persistence::ApiKeyRepositoryPg::new(pool.clone()),
    );

    let jobs_pg = Arc::new(JobsRepositoryPg::new(pool.clone()));
    let jobs_repo: Arc<dyn JobsRepository> = jobs_pg.clone();
    let jobs_enqueuer: Arc<dyn JobsEnqueuer> = jobs_pg;
    let jobs_handle = JobRunner::new(jobs_repo.clone(), JobRunnerConfig::default()).spawn();

    let outbox_pg = Arc::new(OutboxRepositoryPg::new(pool.clone()));
    let outbox_repo: Arc<dyn OutboxRepository> = outbox_pg.clone();
    let outbox_appender: Arc<dyn OutboxAppender> = outbox_pg;
    let outbox_handle = OutboxRelayer::new(
        pool.clone(),
        outbox_repo,
        jobs_repo,
        OutboxRelayerConfig::default(),
    )
    .spawn();

    let state = AppState {
        audit_recorder,
        list_audit_events,
        organisations,
        roles,
        inbound_channels,
        users,
        tokens,
        service_accounts,
        api_keys,
        jobs: jobs_enqueuer,
        outbox: outbox_appender,
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
    let api_key_verifier = crate::auth::middleware::ApiKeyVerifier::pg(
        state.api_keys.clone(),
        state.service_accounts.clone(),
    );
    let auth_layer = AuthLayer::new(
        cfg.jwt_secret.clone(),
        cfg.jwt_issuer.clone(),
        PermissionLoader::pg(pool.clone()),
        RevocationChecker::pg(pool.clone()),
        api_key_verifier,
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
    let cors = build_cors(&cfg)?;
    let router = public
        .merge(protected)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    Ok(AppHandles {
        router,
        audit: audit_handle,
        jobs: jobs_handle,
        outbox: outbox_handle,
    })
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

fn build_cors(cfg: &AppConfig) -> anyhow::Result<CorsLayer> {
    if cfg.cors_allowed_origins.trim().is_empty() {
        anyhow::bail!("EGRAS_CORS_ALLOWED_ORIGINS must be set (comma-separated origins or \"*\")");
    }
    let origins: Vec<axum::http::HeaderValue> = cfg
        .cors_allowed_origins
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if origins.is_empty() {
        anyhow::bail!("EGRAS_CORS_ALLOWED_ORIGINS contains no valid origins");
    }
    Ok(CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]))
}
