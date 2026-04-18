use std::{future::Future, pin::Pin, sync::Arc, task::{Context, Poll}};

use async_trait::async_trait;
use axum::{body::Body, http::{Request, Response, StatusCode, header}, response::IntoResponse};
use sqlx::PgPool;
use tower::{Layer, Service};
use uuid::Uuid;

use crate::auth::jwt::decode_access_token;
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

/// Strategy for loading permissions for a `(user_id, organisation_id)` pair.
#[async_trait]
pub trait PermissionLoaderStrategy: Send + Sync + 'static {
    async fn load(&self, user_id: Uuid, organisation_id: Uuid) -> anyhow::Result<Vec<String>>;
}

/// Wrapper so the layer can hold either a DB-backed or static implementation.
#[derive(Clone)]
pub struct PermissionLoader(Arc<dyn PermissionLoaderStrategy>);

impl PermissionLoader {
    pub fn new<T: PermissionLoaderStrategy>(inner: T) -> Self {
        Self(Arc::new(inner))
    }

    pub fn pg(pool: PgPool) -> Self {
        Self::new(PgPermissionLoader { pool })
    }

    pub fn static_codes(codes: Vec<String>) -> Self {
        Self::new(StaticPermissionLoader { codes: Arc::new(codes) })
    }

    pub async fn load(&self, user: Uuid, org: Uuid) -> anyhow::Result<Vec<String>> {
        self.0.load(user, org).await
    }
}

pub struct PgPermissionLoader { pool: PgPool }

#[async_trait]
impl PermissionLoaderStrategy for PgPermissionLoader {
    async fn load(&self, user_id: Uuid, organisation_id: Uuid) -> anyhow::Result<Vec<String>> {
        let codes: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT p.code
            FROM user_organisation_roles uor
            JOIN role_permissions rp ON rp.role_id = uor.role_id
            JOIN permissions p       ON p.id       = rp.permission_id
            WHERE uor.user_id = $1 AND uor.organisation_id = $2
            "#,
        )
        .bind(user_id)
        .bind(organisation_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(codes)
    }
}

pub struct StaticPermissionLoader { codes: Arc<Vec<String>> }

#[async_trait]
impl PermissionLoaderStrategy for StaticPermissionLoader {
    async fn load(&self, _user: Uuid, _org: Uuid) -> anyhow::Result<Vec<String>> {
        Ok(self.codes.as_ref().clone())
    }
}

#[derive(Clone)]
pub struct AuthLayer {
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
}

impl AuthLayer {
    pub fn new(secret: String, issuer: String, loader: PermissionLoader) -> Self {
        Self { secret: Arc::new(secret), issuer: Arc::new(issuer), loader }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        AuthService { inner, secret: self.secret.clone(), issuer: self.issuer.clone(), loader: self.loader.clone() }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + Into<axum::BoxError> + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let secret = self.secret.clone();
        let issuer = self.issuer.clone();
        let loader = self.loader.clone();

        Box::pin(async move {
            // Extract bearer token
            let token = match req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
                Some(h) if h.starts_with("Bearer ") => h["Bearer ".len()..].to_string(),
                _ => {
                    return Ok(AppError::Unauthenticated { reason: "missing_bearer".into() }
                        .into_response());
                }
            };

            // Decode
            let claims = match decode_access_token(&secret, &issuer, &token) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(AppError::Unauthenticated { reason: "invalid_token".into() }
                        .into_response());
                }
            };

            // Load permissions
            let codes = match loader.load(claims.sub, claims.org).await {
                Ok(c) => c,
                Err(err) => {
                    tracing::error!(error = %err, "permission loader failed");
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
                }
            };
            let perms = PermissionSet::from_codes(codes);

            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(perms);

            inner.call(req).await
        })
    }
}
