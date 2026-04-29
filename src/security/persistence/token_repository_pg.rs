use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::token_repository::{TokenRepoError, TokenRepository};
use crate::security::model::PasswordResetToken;

const MAX_PENDING_TOKENS_PER_USER: i64 = 3;

pub struct TokenRepositoryPg {
    pool: PgPool,
}

impl TokenRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TokenRepository for TokenRepositoryPg {
    async fn insert(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<PasswordResetToken, TokenRepoError> {
        // Enforce cap: delete oldest if already at limit
        let pending = self.count_pending_for_user(user_id).await?;
        if pending >= MAX_PENDING_TOKENS_PER_USER {
            sqlx::query(
                "DELETE FROM password_reset_tokens \
                 WHERE id = ( \
                     SELECT id FROM password_reset_tokens \
                     WHERE user_id = $1 AND consumed_at IS NULL AND expires_at > NOW() \
                     ORDER BY created_at ASC LIMIT 1 \
                 )",
            )
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        }

        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, TokenRow>(
            "INSERT INTO password_reset_tokens (id, user_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, token_hash, user_id, expires_at, consumed_at, created_at",
        )
        .bind(id)
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }

    async fn find_valid(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordResetToken>, TokenRepoError> {
        let row = sqlx::query_as::<_, TokenRow>(
            "SELECT id, token_hash, user_id, expires_at, consumed_at, created_at \
             FROM password_reset_tokens \
             WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > NOW()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn consume(&self, token_id: Uuid) -> Result<(), TokenRepoError> {
        sqlx::query("UPDATE password_reset_tokens SET consumed_at = NOW() WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count_pending_for_user(&self, user_id: Uuid) -> Result<i64, TokenRepoError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM password_reset_tokens \
             WHERE user_id = $1 AND consumed_at IS NULL AND expires_at > NOW()",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    token_hash: String,
    user_id: Uuid,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<TokenRow> for PasswordResetToken {
    fn from(r: TokenRow) -> Self {
        PasswordResetToken {
            id: r.id,
            token_hash: r.token_hash,
            user_id: r.user_id,
            expires_at: r.expires_at,
            used_at: r.consumed_at,
            created_at: r.created_at,
        }
    }
}
