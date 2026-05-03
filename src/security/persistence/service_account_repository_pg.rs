use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::service_account_repository::{
    NewServiceAccount, ServiceAccountRepoError, ServiceAccountRepository,
};
use crate::security::model::ServiceAccount;

pub struct ServiceAccountRepositoryPg {
    pool: PgPool,
}

impl ServiceAccountRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

type SaRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    DateTime<Utc>,
    Uuid,
    Option<DateTime<Utc>>,
);

fn row_to_sa(r: SaRow) -> ServiceAccount {
    ServiceAccount {
        user_id: r.0,
        organisation_id: r.1,
        name: r.2,
        description: r.3,
        created_at: r.4,
        created_by: r.5,
        last_used_at: r.6,
    }
}

#[async_trait]
impl ServiceAccountRepository for ServiceAccountRepositoryPg {
    async fn create(
        &self,
        req: NewServiceAccount,
    ) -> Result<ServiceAccount, ServiceAccountRepoError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ServiceAccountRepoError::Other(e.into()))?;

        let user_id = Uuid::now_v7();
        // Synthesised username/email keep existing UNIQUE constraints happy.
        // password_hash = '!' is a sentinel that never argon2-verifies.
        let synth_username = format!("sa_{user_id}");
        let synth_email = format!("sa_{user_id}@service-account.invalid");

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, kind) \
             VALUES ($1, $2, $3, '!', 'service_account')",
        )
        .bind(user_id)
        .bind(&synth_username)
        .bind(&synth_email)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServiceAccountRepoError::Other(anyhow::Error::from(e)))?;

        let row = sqlx::query_as::<_, SaRow>(
            "INSERT INTO service_accounts \
                 (user_id, organisation_id, name, description, created_by) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING user_id, organisation_id, name, description, \
                       created_at, created_by, last_used_at",
        )
        .bind(user_id)
        .bind(req.organisation_id)
        .bind(&req.name)
        .bind(&req.description)
        .bind(req.created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23505") {
                    return ServiceAccountRepoError::DuplicateName;
                }
            }
            ServiceAccountRepoError::Other(anyhow::Error::from(e))
        })?;

        tx.commit()
            .await
            .map_err(|e| ServiceAccountRepoError::Other(e.into()))?;
        Ok(row_to_sa(row))
    }

    async fn find(
        &self,
        organisation_id: Uuid,
        sa_user_id: Uuid,
    ) -> anyhow::Result<Option<ServiceAccount>> {
        let row = sqlx::query_as::<_, SaRow>(
            "SELECT user_id, organisation_id, name, description, \
                    created_at, created_by, last_used_at \
             FROM service_accounts \
             WHERE organisation_id = $1 AND user_id = $2",
        )
        .bind(organisation_id)
        .bind(sa_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_sa))
    }

    async fn list(
        &self,
        organisation_id: Uuid,
        limit: u32,
        after: Option<(DateTime<Utc>, Uuid)>,
    ) -> anyhow::Result<Vec<ServiceAccount>> {
        let rows = match after {
            None => {
                sqlx::query_as::<_, SaRow>(
                    "SELECT user_id, organisation_id, name, description, \
                            created_at, created_by, last_used_at \
                     FROM service_accounts \
                     WHERE organisation_id = $1 \
                     ORDER BY created_at ASC, user_id ASC \
                     LIMIT $2",
                )
                .bind(organisation_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            Some((c, id)) => {
                sqlx::query_as::<_, SaRow>(
                    "SELECT user_id, organisation_id, name, description, \
                            created_at, created_by, last_used_at \
                     FROM service_accounts \
                     WHERE organisation_id = $1 \
                       AND (created_at, user_id) > ($2, $3) \
                     ORDER BY created_at ASC, user_id ASC \
                     LIMIT $4",
                )
                .bind(organisation_id)
                .bind(c)
                .bind(id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_sa).collect())
    }

    async fn delete(&self, organisation_id: Uuid, sa_user_id: Uuid) -> anyhow::Result<bool> {
        // Verify the SA lives in this org first; only then DELETE the user row
        // (cascades to service_accounts + api_keys).
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM service_accounts \
             WHERE organisation_id = $1 AND user_id = $2",
        )
        .bind(organisation_id)
        .bind(sa_user_id)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            return Ok(false);
        }
        let res = sqlx::query("DELETE FROM users WHERE id = $1 AND kind = 'service_account'")
            .bind(sa_user_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn touch_last_used(&self, sa_user_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE service_accounts \
                SET last_used_at = NOW() \
              WHERE user_id = $1 \
                AND (last_used_at IS NULL \
                     OR last_used_at < NOW() - INTERVAL '60 seconds')",
        )
        .bind(sa_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
