//! Test helpers. Enabled via the `testing` feature or in test builds.

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditRepository, AuditRepositoryPg};
use crate::audit::service::{AuditRecorder, ListAuditEventsImpl, ListAuditEvents, RecorderError};
use crate::auth::jwt::encode_access_token;
use crate::db::run_migrations;

/// Ephemeral Postgres for tests. Keep the `ContainerAsync` alive for the test's lifetime.
pub struct TestPool {
    pub pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestPool {
    pub async fn fresh() -> Self {
        let container = Postgres::default()
            .with_db_name("egras_test")
            .with_user("egras")
            .with_password("egras")
            .start()
            .await
            .expect("start postgres container");

        let host_port = container.get_host_port_ipv4(5432).await.expect("pg port");
        let url = format!("postgres://egras:egras@127.0.0.1:{host_port}/egras_test");
        let pool = PgPool::connect(&url).await.expect("connect pg");
        run_migrations(&pool).await.expect("migrations");
        Self { pool, _container: container }
    }
}

/// Synchronous audit recorder for E2E tests — writes directly to the DB so the
/// rows are visible to the next query without waiting for the worker.
pub struct BlockingAuditRecorder {
    repo: Arc<dyn AuditRepository>,
    /// Captures events for assertion when DB is not required.
    pub captured: Arc<Mutex<Vec<AuditEvent>>>,
}

impl BlockingAuditRecorder {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self {
        Self { repo, captured: Arc::new(Mutex::new(Vec::new())) }
    }
}

#[async_trait]
impl AuditRecorder for BlockingAuditRecorder {
    async fn record(&self, event: AuditEvent) -> Result<(), RecorderError> {
        self.captured.lock().await.push(event.clone());
        self.repo.insert(&event).await.map_err(|e| {
            tracing::error!(error = %e, "BlockingAuditRecorder insert failed");
            RecorderError::Closed
        })?;
        Ok(())
    }
}

/// Issue a JWT for tests. Caller owns the permission loading path — see `MockAppStateBuilder`.
pub fn mint_jwt(secret: &str, issuer: &str, user_id: Uuid, org_id: Uuid, ttl_secs: i64) -> String {
    encode_access_token(secret, issuer, user_id, org_id, ttl_secs)
        .expect("mint_jwt failed")
}

/// Builder that produces an `AppState` wired with audit infra for tests. Plan 2
/// extends this with fluent setters for domain service mocks.
pub struct MockAppStateBuilder {
    pool: PgPool,
    audit_recorder: Option<Arc<dyn AuditRecorder>>,
    list_audit_events: Option<Arc<dyn ListAuditEvents>>,
}

impl MockAppStateBuilder {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, audit_recorder: None, list_audit_events: None }
    }

    pub fn with_blocking_audit(mut self) -> Self {
        let repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(self.pool.clone()));
        self.audit_recorder = Some(Arc::new(BlockingAuditRecorder::new(repo.clone())));
        self.list_audit_events = Some(Arc::new(ListAuditEventsImpl::new(repo)));
        self
    }

    pub fn audit_recorder(mut self, rec: Arc<dyn AuditRecorder>) -> Self {
        self.audit_recorder = Some(rec); self
    }

    pub fn list_audit_events(mut self, svc: Arc<dyn ListAuditEvents>) -> Self {
        self.list_audit_events = Some(svc); self
    }

    pub fn build(self) -> AppState {
        AppState {
            pool: self.pool,
            audit_recorder: self.audit_recorder.expect("audit_recorder not set"),
            list_audit_events: self.list_audit_events.expect("list_audit_events not set"),
        }
    }
}

/// A running test server. Holds the join handle and a shutdown sender.
pub struct TestApp {
    pub base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl TestApp {
    /// Spawn `build_app` bound to port 0. Returns base URL "http://127.0.0.1:<port>".
    pub async fn spawn(pool: PgPool, cfg: crate::config::AppConfig) -> Self {
        let (router, audit_handle) = crate::build_app(pool, cfg).await.expect("build_app");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let base_url = format!("http://{addr}");

        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move { rx.await.ok(); });
            let _ = server.await;
            audit_handle.shutdown().await;
        });

        Self { base_url, shutdown: Some(tx), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() { let _ = tx.send(()); }
        if let Some(h) = self.handle.take() { let _ = h.await; }
    }
}
