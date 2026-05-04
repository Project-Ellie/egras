use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::features::persistence::{FeatureRepoError, FeatureRepository};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum EvaluateError {
    #[error("unknown feature slug")]
    UnknownSlug,
    #[error(transparent)]
    Repo(#[from] FeatureRepoError),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait FeatureEvaluator: Send + Sync + 'static {
    async fn evaluate(&self, org: Uuid, slug: &str) -> Result<Value, EvaluateError>;
    async fn invalidate(&self, org: Uuid, slug: &str);
    async fn invalidate_all(&self);
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

struct CachedValue {
    value: Value,
    expires_at: Instant,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

pub struct PgFeatureEvaluator {
    repo: Arc<dyn FeatureRepository>,
    cache: Arc<RwLock<HashMap<(Uuid, String), CachedValue>>>,
    ttl: Duration,
}

impl PgFeatureEvaluator {
    /// Default TTL of 60 seconds.
    pub fn new(repo: Arc<dyn FeatureRepository>) -> Self {
        Self {
            repo,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(60),
        }
    }

    /// Override TTL (useful in tests to verify cache expiry behaviour).
    #[cfg(any(test, feature = "testing"))]
    pub fn with_ttl(repo: Arc<dyn FeatureRepository>, ttl: Duration) -> Self {
        Self {
            repo,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }
}

#[async_trait]
impl FeatureEvaluator for PgFeatureEvaluator {
    async fn evaluate(&self, org: Uuid, slug: &str) -> Result<Value, EvaluateError> {
        let now = Instant::now();
        let key = (org, slug.to_string());

        // --- Read lock: cache hit? ---
        {
            let read = self.cache.read().await;
            if let Some(entry) = read.get(&key) {
                if entry.expires_at > now {
                    return Ok(entry.value.clone());
                }
            }
        }

        // --- Cache miss: hit the repo ---
        let value = if let Some(ov) = self.repo.get_override(org, slug).await? {
            ov.value
        } else if let Some(def) = self.repo.get_definition(slug).await? {
            def.default_value
        } else {
            // Unknown slug — do NOT cache.
            return Err(EvaluateError::UnknownSlug);
        };

        // --- Write lock: insert into cache ---
        {
            let mut write = self.cache.write().await;
            write.insert(
                key,
                CachedValue {
                    value: value.clone(),
                    expires_at: now + self.ttl,
                },
            );
        }

        Ok(value)
    }

    async fn invalidate(&self, org: Uuid, slug: &str) {
        let mut write = self.cache.write().await;
        write.remove(&(org, slug.to_string()));
    }

    async fn invalidate_all(&self) {
        let mut write = self.cache.write().await;
        write.clear();
    }
}
