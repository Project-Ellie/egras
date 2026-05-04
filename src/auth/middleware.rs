use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use sqlx::PgPool;
use tower::{Layer, Service};
use uuid::Uuid;

use crate::auth::extractors::Caller;
use crate::auth::jwt::{decode_access_token, Claims};
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;
use crate::features::FeatureEvaluator;

/// Identifies which HTTP header carried the API-key credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderSource {
    XApiKey,
    AuthorizationBearer,
}

impl HeaderSource {
    /// The slug used in the `auth.api_key_headers` allowlist.
    fn slug(self) -> &'static str {
        match self {
            Self::XApiKey => "x-api-key",
            Self::AuthorizationBearer => "authorization-bearer",
        }
    }
}

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
        Self::new(StaticPermissionLoader {
            codes: Arc::new(codes),
        })
    }

    pub async fn load(&self, user: Uuid, org: Uuid) -> anyhow::Result<Vec<String>> {
        self.0.load(user, org).await
    }
}

pub struct PgPermissionLoader {
    pool: PgPool,
}

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

pub struct StaticPermissionLoader {
    codes: Arc<Vec<String>>,
}

#[async_trait]
impl PermissionLoaderStrategy for StaticPermissionLoader {
    async fn load(&self, _user: Uuid, _org: Uuid) -> anyhow::Result<Vec<String>> {
        Ok(self.codes.as_ref().clone())
    }
}

/// Strategy for checking whether a JWT JTI has been revoked.
#[async_trait]
pub trait RevocationStrategy: Send + Sync + 'static {
    async fn is_revoked(&self, jti: Uuid) -> anyhow::Result<bool>;
}

/// Wrapper so the layer can hold either a DB-backed or no-op implementation.
#[derive(Clone)]
pub struct RevocationChecker(Arc<dyn RevocationStrategy>);

impl RevocationChecker {
    pub fn new<T: RevocationStrategy>(inner: T) -> Self {
        Self(Arc::new(inner))
    }

    pub fn pg(pool: PgPool) -> Self {
        Self::new(PgRevocationChecker { pool })
    }

    /// Never-revoked checker for tests that don't exercise logout.
    pub fn none() -> Self {
        Self::new(NoRevocationChecker)
    }

    pub async fn is_revoked(&self, jti: Uuid) -> anyhow::Result<bool> {
        self.0.is_revoked(jti).await
    }
}

pub struct PgRevocationChecker {
    pool: PgPool,
}

#[async_trait]
impl RevocationStrategy for PgRevocationChecker {
    async fn is_revoked(&self, jti: Uuid) -> anyhow::Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS( \
                 SELECT 1 FROM revoked_tokens \
                 WHERE jti = $1 AND expires_at > NOW() \
             )",
        )
        .bind(jti)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }
}

pub struct NoRevocationChecker;

#[async_trait]
impl RevocationStrategy for NoRevocationChecker {
    async fn is_revoked(&self, _jti: Uuid) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Result of an API-key Bearer verification: prefix lookup hit + secret
/// matched the stored argon2 hash.
#[derive(Debug, Clone)]
pub struct VerifiedKey {
    pub key_id: Uuid,
    pub sa_user_id: Uuid,
    pub organisation_id: Uuid,
    pub scopes: Option<Vec<String>>,
}

/// Strategy for verifying an `egras_*` Bearer credential.
///
/// Implementations look the prefix up in `api_keys`, verify the secret with
/// constant-time hash comparison, and return the SA's identity + per-key
/// scope list. `None` means either unknown prefix or bad secret — same 401.
#[async_trait]
pub trait ApiKeyVerifierStrategy: Send + Sync + 'static {
    async fn verify(&self, prefix: &str, secret: &str) -> anyhow::Result<Option<VerifiedKey>>;
    /// Best-effort: update `last_used_at` on the key + its SA, throttled to 60 s.
    /// Errors are logged but not propagated.
    async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid);
}

#[derive(Clone)]
pub struct ApiKeyVerifier(Arc<dyn ApiKeyVerifierStrategy>);

impl ApiKeyVerifier {
    pub fn new<T: ApiKeyVerifierStrategy>(inner: T) -> Self {
        Self(Arc::new(inner))
    }
    pub async fn verify(&self, prefix: &str, secret: &str) -> anyhow::Result<Option<VerifiedKey>> {
        self.0.verify(prefix, secret).await
    }
    pub async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid) {
        self.0.touch_last_used(key_id, sa_user_id).await;
    }
}

/// No-op verifier that always returns `None`. Useful in tests that don't
/// exercise the API-key path.
pub struct NoApiKeyVerifier;

#[async_trait]
impl ApiKeyVerifierStrategy for NoApiKeyVerifier {
    async fn verify(&self, _prefix: &str, _secret: &str) -> anyhow::Result<Option<VerifiedKey>> {
        Ok(None)
    }
    async fn touch_last_used(&self, _key_id: Uuid, _sa_user_id: Uuid) {}
}

#[derive(Clone)]
pub struct AuthLayer {
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
    revocation: RevocationChecker,
    api_keys: ApiKeyVerifier,
    features: Arc<dyn FeatureEvaluator>,
}

impl AuthLayer {
    pub fn new(
        secret: String,
        issuer: String,
        loader: PermissionLoader,
        revocation: RevocationChecker,
        api_keys: ApiKeyVerifier,
        features: Arc<dyn FeatureEvaluator>,
    ) -> Self {
        Self {
            secret: Arc::new(secret),
            issuer: Arc::new(issuer),
            loader,
            revocation,
            api_keys,
            features,
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            secret: self.secret.clone(),
            issuer: self.issuer.clone(),
            loader: self.loader.clone(),
            revocation: self.revocation.clone(),
            api_keys: self.api_keys.clone(),
            features: self.features.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    secret: Arc<String>,
    issuer: Arc<String>,
    loader: PermissionLoader,
    revocation: RevocationChecker,
    api_keys: ApiKeyVerifier,
    features: Arc<dyn FeatureEvaluator>,
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
        let revocation = self.revocation.clone();
        let api_keys = self.api_keys.clone();
        let features = self.features.clone();

        Box::pin(async move {
            // ── Step 1: Detect credential source ─────────────────────────────
            //
            // `X-API-Key` takes precedence over `Authorization: Bearer`.
            // If X-API-Key is present, the token MUST be an API key — a JWT
            // in that header is rejected immediately.
            // If only Authorization: Bearer is present, the token may be
            // either an API key or a JWT (sniffed by the `egras_` prefix).

            let x_api_key_token = req
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| (s.to_string(), HeaderSource::XApiKey));

            let bearer_token = if x_api_key_token.is_some() {
                None // X-API-Key takes precedence
            } else {
                req.headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string))
                    .map(|t| (t, HeaderSource::AuthorizationBearer))
            };

            let (token, header_source) = match x_api_key_token.or(bearer_token) {
                Some(pair) => pair,
                None => {
                    return Ok(AppError::Unauthenticated {
                        reason: "missing_credentials".into(),
                    }
                    .into_response());
                }
            };

            // ── Step 2: X-API-Key path ────────────────────────────────────────
            //
            // X-API-Key MUST carry an API key — if `parse` returns None the
            // token is not in `egras_<prefix>.<secret>` format → reject.

            if header_source == HeaderSource::XApiKey {
                let parsed = match crate::security::service::api_key_secret::parse(&token) {
                    Some(p) => p,
                    None => {
                        return Ok(AppError::Unauthenticated {
                            reason: "invalid_api_key".into(),
                        }
                        .into_response());
                    }
                };

                return handle_api_key(
                    ApiKeyCtx {
                        prefix: parsed.prefix,
                        secret: parsed.secret,
                        header_source: HeaderSource::XApiKey,
                        api_keys: &api_keys,
                        features: &features,
                        loader: &loader,
                        issuer: &issuer,
                    },
                    &mut inner,
                    req,
                )
                .await;
            }

            // ── Step 3: Authorization: Bearer path ────────────────────────────
            //
            // Token may be an API key (sniffed by prefix) OR a JWT.

            if let Some(parsed) = crate::security::service::api_key_secret::parse(&token) {
                return handle_api_key(
                    ApiKeyCtx {
                        prefix: parsed.prefix,
                        secret: parsed.secret,
                        header_source: HeaderSource::AuthorizationBearer,
                        api_keys: &api_keys,
                        features: &features,
                        loader: &loader,
                        issuer: &issuer,
                    },
                    &mut inner,
                    req,
                )
                .await;
            }

            // ── Step 4: JWT path ──────────────────────────────────────────────
            //
            // The allowlist flag does NOT apply here — the flag governs API-key
            // transport only.

            let claims = match decode_access_token(&secret, &issuer, &token) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(AppError::Unauthenticated {
                        reason: "invalid_token".into(),
                    }
                    .into_response());
                }
            };

            match revocation.is_revoked(claims.jti).await {
                Ok(true) => {
                    return Ok(AppError::Unauthenticated {
                        reason: "token_revoked".into(),
                    }
                    .into_response());
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::error!(error = %err, "revocation check failed");
                    return Ok(
                        (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
                    );
                }
            }

            let codes = match loader.load(claims.sub, claims.org).await {
                Ok(c) => c,
                Err(err) => {
                    tracing::error!(error = %err, "permission loader failed");
                    return Ok(
                        (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
                    );
                }
            };
            let perms = PermissionSet::from_codes(codes);
            let caller = Caller::User {
                user_id: claims.sub,
                org_id: claims.org,
                jti: claims.jti,
            };

            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(perms);
            req.extensions_mut().insert(caller);

            inner.call(req).await
        })
    }
}

/// Dependencies threaded into [`handle_api_key`] to stay under clippy's
/// `too_many_arguments` limit.
struct ApiKeyCtx<'a> {
    prefix: &'a str,
    secret: &'a str,
    header_source: HeaderSource,
    api_keys: &'a ApiKeyVerifier,
    features: &'a Arc<dyn FeatureEvaluator>,
    loader: &'a PermissionLoader,
    issuer: &'a str,
}

/// Shared logic for the API-key path (used by both header sources).
///
/// 1. Verifies the key prefix + secret.
/// 2. Evaluates `auth.api_key_headers` for the org of the verified key.
/// 3. Rejects if `header_source` is not in the allowlist.
/// 4. Loads permissions, synthesises Claims, inserts extensions, forwards.
async fn handle_api_key<S>(
    ctx: ApiKeyCtx<'_>,
    inner: &mut S,
    mut req: Request<Body>,
) -> Result<Response<Body>, S::Error>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    let ApiKeyCtx {
        prefix,
        secret,
        header_source,
        api_keys,
        features,
        loader,
        issuer,
    } = ctx;
    // Verify prefix + secret.
    let verified = match api_keys.verify(prefix, secret).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Ok(AppError::Unauthenticated {
                reason: "invalid_api_key".into(),
            }
            .into_response());
        }
        Err(err) => {
            tracing::error!(error = %err, "api key verifier failed");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
        }
    };

    // Evaluate the per-org allowlist AFTER we have the org_id.
    let allowlist_value = match features
        .evaluate(verified.organisation_id, "auth.api_key_headers")
        .await
    {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(error = %err, "feature evaluator failed (auth.api_key_headers)");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
        }
    };

    let allowed: Vec<String> = match allowlist_value.as_array() {
        Some(arr) => {
            let total = arr.len();
            let allowed: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if allowed.len() < total {
                tracing::warn!(
                    org_id = %verified.organisation_id,
                    dropped = total - allowed.len(),
                    "auth.api_key_headers contains non-string entries; dropping them"
                );
            }
            allowed
        }
        None => {
            tracing::error!(value = %allowlist_value, "auth.api_key_headers flag is not an array");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
        }
    };

    if !allowed.iter().any(|h| h == header_source.slug()) {
        tracing::warn!(
            org_id = %verified.organisation_id,
            key_id = %verified.key_id,
            header_source = header_source.slug(),
            "api key rejected: header source not in org allowlist"
        );
        return Ok(AppError::Unauthenticated {
            reason: format!("api_key_header_not_allowed:{}", header_source.slug()),
        }
        .into_response());
    }

    // Load permissions and synthesise Claims.
    let codes = match loader
        .load(verified.sa_user_id, verified.organisation_id)
        .await
    {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(error = %err, "permission loader failed (api key path)");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response());
        }
    };
    let mut perms = PermissionSet::from_codes(codes);
    if let Some(scopes) = verified.scopes.as_ref() {
        perms = perms.intersect(scopes);
    }

    // Best-effort throttled last_used touch — fire-and-forget.
    let api_keys2 = api_keys.clone();
    let key_id_for_touch = verified.key_id;
    let sa_for_touch = verified.sa_user_id;
    tokio::spawn(async move {
        api_keys2
            .touch_last_used(key_id_for_touch, sa_for_touch)
            .await;
    });

    // Synthesised Claims for downstream handlers using the existing
    // AuthedCaller / Perm<P> extractors. jti is deterministic from
    // the api_key.id; exp far in the future (never re-validated post-
    // middleware on the api-key path).
    let now = chrono::Utc::now().timestamp();
    let synth_jti = Uuid::from_u128(verified.key_id.as_u128() ^ 0xA1A1_A1A1_u128);
    let claims = Claims {
        sub: verified.sa_user_id,
        org: verified.organisation_id,
        iat: now,
        exp: now + 365 * 24 * 3600,
        jti: synth_jti,
        iss: issuer.to_string(),
        typ: "access".to_string(),
    };

    req.extensions_mut().insert(claims);
    req.extensions_mut().insert(perms);
    req.extensions_mut().insert(Caller::ApiKey {
        key_id: verified.key_id,
        sa_user_id: verified.sa_user_id,
        org_id: verified.organisation_id,
    });
    inner.call(req).await
}
