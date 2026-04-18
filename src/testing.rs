//! Test helpers. Enabled via the `testing` feature or in test builds.

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{Executor, PgPool};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditRepository, AuditRepositoryPg};
use crate::audit::service::{AuditRecorder, ListAuditEvents, ListAuditEventsImpl, RecorderError};
use crate::auth::jwt::encode_access_token;
use crate::db::run_migrations;

/// Returns the admin database URL used to create per-test databases.
///
/// Honours `TEST_DATABASE_URL` (set by docker-compose / CI service); falls
/// back to the local test-pg container bound on `localhost:15432`.
fn admin_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://egras:egras@127.0.0.1:15432/postgres".to_string())
}

/// A test database carved out of a shared postgres instance. On drop the
/// database is left in place — cleanup is best-effort; spin the shared
/// container from scratch for a clean slate.
pub struct TestPool {
    pub pool: PgPool,
    pub db_name: String,
}

impl TestPool {
    pub async fn fresh() -> Self {
        let admin = admin_url();
        let suffix = Uuid::now_v7().simple().to_string();
        let db_name = format!("egras_test_{suffix}");

        let admin_pool = PgPool::connect(&admin)
            .await
            .expect("connect admin pg — is the shared test-pg container running?");
        admin_pool
            .execute(format!(r#"CREATE DATABASE "{db_name}""#).as_str())
            .await
            .expect("create test database");
        admin_pool.close().await;

        // Rebuild URL with the new database name.
        let parsed = url::Url::parse(&admin).expect("parse admin url");
        let scheme = parsed.scheme();
        let user = parsed.username();
        let pw = parsed.password().unwrap_or("");
        let host = parsed.host_str().expect("admin url has host");
        let port = parsed.port().unwrap_or(5432);
        let url = format!("{scheme}://{user}:{pw}@{host}:{port}/{db_name}");

        let pool = PgPool::connect(&url).await.expect("connect test pg");
        run_migrations(&pool).await.expect("migrations");
        Self { pool, db_name }
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
        Self {
            repo,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
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
    encode_access_token(secret, issuer, user_id, org_id, ttl_secs).expect("mint_jwt failed")
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
        Self {
            pool,
            audit_recorder: None,
            list_audit_events: None,
        }
    }

    pub fn with_blocking_audit(mut self) -> Self {
        let repo: Arc<dyn AuditRepository> = Arc::new(AuditRepositoryPg::new(self.pool.clone()));
        self.audit_recorder = Some(Arc::new(BlockingAuditRecorder::new(repo.clone())));
        self.list_audit_events = Some(Arc::new(ListAuditEventsImpl::new(repo)));
        self
    }

    pub fn audit_recorder(mut self, rec: Arc<dyn AuditRecorder>) -> Self {
        self.audit_recorder = Some(rec);
        self
    }

    pub fn list_audit_events(mut self, svc: Arc<dyn ListAuditEvents>) -> Self {
        self.list_audit_events = Some(svc);
        self
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

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let base_url = format!("http://{addr}");

        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                rx.await.ok();
            });
            let _ = server.await;
            audit_handle.shutdown().await;
        });

        Self {
            base_url,
            shutdown: Some(tx),
            handle: Some(handle),
        }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }
}
