use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::api_key_repository::{ApiKeyRepoError, ApiKeyRepository, ApiKeyRow, NewApiKeyRow};
use super::service_account_repository::ServiceAccountRepository;
use crate::auth::middleware::{ApiKeyVerifier, ApiKeyVerifierStrategy, VerifiedKey};
use crate::security::model::ApiKey;

pub struct ApiKeyRepositoryPg {
    pool: PgPool,
}

impl ApiKeyRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

type KeyRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<Vec<String>>,
    DateTime<Utc>,
    Uuid,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

fn row_to_key(r: KeyRow) -> ApiKey {
    ApiKey {
        id: r.0,
        service_account_user_id: r.1,
        prefix: r.2,
        name: r.3,
        scopes: r.4,
        created_at: r.5,
        created_by: r.6,
        last_used_at: r.7,
        revoked_at: r.8,
    }
}

type VerifyRow = (
    // ApiKey columns:
    Uuid,
    Uuid,
    String,
    String,
    Option<Vec<String>>,
    DateTime<Utc>,
    Uuid,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    // secret + org join:
    String,
    Uuid,
);

async fn insert_key(
    conn: &mut sqlx::PgConnection,
    new: NewApiKeyRow,
) -> Result<ApiKey, ApiKeyRepoError> {
    let row = sqlx::query_as::<_, KeyRow>(
        "INSERT INTO api_keys \
             (id, service_account_user_id, prefix, secret_hash, name, scopes, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, service_account_user_id, prefix, name, scopes, \
                   created_at, created_by, last_used_at, revoked_at",
    )
    .bind(new.id)
    .bind(new.service_account_user_id)
    .bind(&new.prefix)
    .bind(&new.secret_hash)
    .bind(&new.name)
    .bind(&new.scopes)
    .bind(new.created_by)
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref dbe) = e {
            if dbe.code().as_deref() == Some("23505") {
                return ApiKeyRepoError::DuplicatePrefix;
            }
        }
        ApiKeyRepoError::Other(anyhow::Error::from(e))
    })?;
    Ok(row_to_key(row))
}

#[async_trait]
impl ApiKeyRepository for ApiKeyRepositoryPg {
    async fn create(&self, req: NewApiKeyRow) -> Result<ApiKey, ApiKeyRepoError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| ApiKeyRepoError::Other(e.into()))?;
        insert_key(&mut conn, req).await
    }

    async fn find_active_by_prefix(&self, prefix: &str) -> anyhow::Result<Option<ApiKeyRow>> {
        let row = sqlx::query_as::<_, VerifyRow>(
            "SELECT k.id, k.service_account_user_id, k.prefix, k.name, k.scopes, \
                    k.created_at, k.created_by, k.last_used_at, k.revoked_at, \
                    k.secret_hash, sa.organisation_id \
             FROM api_keys k \
             JOIN service_accounts sa ON sa.user_id = k.service_account_user_id \
             WHERE k.prefix = $1 AND k.revoked_at IS NULL",
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            let key = ApiKey {
                id: r.0,
                service_account_user_id: r.1,
                prefix: r.2,
                name: r.3,
                scopes: r.4,
                created_at: r.5,
                created_by: r.6,
                last_used_at: r.7,
                revoked_at: r.8,
            };
            ApiKeyRow {
                key,
                secret_hash: r.9,
                organisation_id: r.10,
            }
        }))
    }

    async fn find(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, KeyRow>(
            "SELECT id, service_account_user_id, prefix, name, scopes, \
                    created_at, created_by, last_used_at, revoked_at \
             FROM api_keys \
             WHERE id = $1 AND service_account_user_id = $2",
        )
        .bind(key_id)
        .bind(sa_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_key))
    }

    async fn list_by_sa(&self, sa_user_id: Uuid) -> anyhow::Result<Vec<ApiKey>> {
        let rows = sqlx::query_as::<_, KeyRow>(
            "SELECT id, service_account_user_id, prefix, name, scopes, \
                    created_at, created_by, last_used_at, revoked_at \
             FROM api_keys \
             WHERE service_account_user_id = $1 \
             ORDER BY created_at ASC, id ASC",
        )
        .bind(sa_user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_key).collect())
    }

    async fn revoke(&self, sa_user_id: Uuid, key_id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE api_keys \
                SET revoked_at = NOW() \
              WHERE id = $1 \
                AND service_account_user_id = $2 \
                AND revoked_at IS NULL",
        )
        .bind(key_id)
        .bind(sa_user_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn touch_last_used(&self, key_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE api_keys \
                SET last_used_at = NOW() \
              WHERE id = $1 \
                AND (last_used_at IS NULL \
                     OR last_used_at < NOW() - INTERVAL '60 seconds')",
        )
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn rotate(&self, old_key_id: Uuid, new: NewApiKeyRow) -> Result<ApiKey, ApiKeyRepoError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ApiKeyRepoError::Other(e.into()))?;

        sqlx::query(
            "UPDATE api_keys \
                SET revoked_at = NOW() \
              WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(old_key_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiKeyRepoError::Other(e.into()))?;

        let key = insert_key(&mut tx, new).await?;

        tx.commit()
            .await
            .map_err(|e| ApiKeyRepoError::Other(e.into()))?;
        Ok(key)
    }
}

/// Postgres-backed implementation of `ApiKeyVerifierStrategy`. Wraps both
/// repositories so the AuthLayer can perform prefix → SA + scope lookup
/// AND the throttled last-used update through the same trait object.
pub struct PgApiKeyVerifier {
    pub api_keys: Arc<dyn ApiKeyRepository>,
    pub service_accounts: Arc<dyn ServiceAccountRepository>,
}

impl PgApiKeyVerifier {
    pub fn new(
        api_keys: Arc<dyn ApiKeyRepository>,
        service_accounts: Arc<dyn ServiceAccountRepository>,
    ) -> Self {
        Self {
            api_keys,
            service_accounts,
        }
    }
}

#[async_trait]
impl ApiKeyVerifierStrategy for PgApiKeyVerifier {
    async fn verify(&self, prefix: &str, secret: &str) -> anyhow::Result<Option<VerifiedKey>> {
        let row = match self.api_keys.find_active_by_prefix(prefix).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        if !crate::security::service::api_key_secret::verify_secret(secret, &row.secret_hash)? {
            return Ok(None);
        }
        Ok(Some(VerifiedKey {
            key_id: row.key.id,
            sa_user_id: row.key.service_account_user_id,
            organisation_id: row.organisation_id,
            scopes: row.key.scopes,
        }))
    }

    async fn touch_last_used(&self, key_id: Uuid, sa_user_id: Uuid) {
        if let Err(e) = self.api_keys.touch_last_used(key_id).await {
            tracing::warn!(error = %e, key_id = %key_id, "api_key touch_last_used failed");
        }
        if let Err(e) = self.service_accounts.touch_last_used(sa_user_id).await {
            tracing::warn!(error = %e, sa_user_id = %sa_user_id, "service_account touch_last_used failed");
        }
    }
}

impl ApiKeyVerifier {
    /// Convenience constructor for the default Pg-backed verifier.
    pub fn pg(
        api_keys: Arc<dyn ApiKeyRepository>,
        service_accounts: Arc<dyn ServiceAccountRepository>,
    ) -> Self {
        Self::new(PgApiKeyVerifier::new(api_keys, service_accounts))
    }
}
