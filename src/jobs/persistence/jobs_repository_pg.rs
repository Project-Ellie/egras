use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

use super::JobsRepository;
use crate::jobs::model::{EnqueueRequest, Job, JobState};
use crate::jobs::runner::JobsEnqueuer;

pub struct JobsRepositoryPg {
    pool: PgPool,
}

impl JobsRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

struct JobRow {
    id: Uuid,
    kind: String,
    payload: serde_json::Value,
    state: String,
    attempts: i32,
    max_attempts: i32,
    run_at: DateTime<Utc>,
    locked_until: Option<DateTime<Utc>>,
    locked_by: Option<String>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<JobRow> for Job {
    type Error = anyhow::Error;
    fn try_from(r: JobRow) -> Result<Self, Self::Error> {
        Ok(Job {
            id: r.id,
            kind: r.kind,
            payload: r.payload,
            state: JobState::from_str(&r.state)?,
            attempts: r.attempts,
            max_attempts: r.max_attempts,
            run_at: r.run_at,
            locked_until: r.locked_until,
            locked_by: r.locked_by,
            last_error: r.last_error,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

#[async_trait]
impl JobsRepository for JobsRepositoryPg {
    async fn enqueue(&self, req: EnqueueRequest) -> anyhow::Result<Uuid> {
        let mut tx = self.pool.begin().await?;
        let id = JobsRepository::enqueue_in_tx(self, &mut tx, req).await?;
        tx.commit().await?;
        Ok(id)
    }

    async fn enqueue_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        req: EnqueueRequest,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO jobs (id, kind, payload, state, attempts, max_attempts, run_at)
            VALUES ($1, $2, $3, 'pending', 0, $4, $5)
            "#,
        )
        .bind(id)
        .bind(&req.kind)
        .bind(&req.payload)
        .bind(req.max_attempts)
        .bind(req.run_at)
        .execute(&mut *tx)
        .await?;
        Ok(id)
    }

    async fn claim_due(
        &self,
        worker_id: &str,
        kinds: &[String],
        visibility: Duration,
        limit: u32,
    ) -> anyhow::Result<Vec<Job>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let visibility_secs = visibility.as_secs() as i64;

        let rows = sqlx::query_as::<_, (
            Uuid,
            String,
            serde_json::Value,
            String,
            i32,
            i32,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
            DateTime<Utc>,
        )>(
            r#"
            WITH due AS (
                SELECT id
                FROM jobs
                WHERE kind = ANY($1)
                  AND (
                        (state = 'pending' AND run_at <= now())
                        OR (state = 'running' AND locked_until IS NOT NULL AND locked_until <= now())
                      )
                ORDER BY run_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            UPDATE jobs j
            SET state        = 'running',
                locked_until = now() + make_interval(secs => $3::double precision),
                locked_by    = $4,
                updated_at   = now()
            FROM due
            WHERE j.id = due.id
            RETURNING
                j.id, j.kind, j.payload, j.state,
                j.attempts, j.max_attempts, j.run_at,
                j.locked_until, j.locked_by, j.last_error,
                j.created_at, j.updated_at
            "#,
        )
        .bind(kinds)
        .bind(limit as i64)
        .bind(visibility_secs)
        .bind(worker_id)
        .fetch_all(&self.pool)
        .await?;

        let mut jobs = Vec::with_capacity(rows.len());
        for r in rows {
            jobs.push(
                JobRow {
                    id: r.0,
                    kind: r.1,
                    payload: r.2,
                    state: r.3,
                    attempts: r.4,
                    max_attempts: r.5,
                    run_at: r.6,
                    locked_until: r.7,
                    locked_by: r.8,
                    last_error: r.9,
                    created_at: r.10,
                    updated_at: r.11,
                }
                .try_into()?,
            );
        }
        Ok(jobs)
    }

    async fn mark_done(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE jobs
               SET state        = 'done',
                   locked_until = NULL,
                   locked_by    = NULL,
                   updated_at   = now()
             WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_failed_retry(
        &self,
        id: Uuid,
        error: &str,
        next_run_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE jobs
               SET state        = 'pending',
                   attempts     = attempts + 1,
                   last_error   = $2,
                   run_at       = $3,
                   locked_until = NULL,
                   locked_by    = NULL,
                   updated_at   = now()
             WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .bind(next_run_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_dead(&self, id: Uuid, error: &str) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE jobs
               SET state        = 'dead',
                   attempts     = attempts + 1,
                   last_error   = $2,
                   locked_until = NULL,
                   locked_by    = NULL,
                   updated_at   = now()
             WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find(&self, id: Uuid) -> anyhow::Result<Option<Job>> {
        let row = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                serde_json::Value,
                String,
                i32,
                i32,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
                Option<String>,
                Option<String>,
                DateTime<Utc>,
                DateTime<Utc>,
            ),
        >(
            r#"
            SELECT id, kind, payload, state, attempts, max_attempts, run_at,
                   locked_until, locked_by, last_error, created_at, updated_at
              FROM jobs
             WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            None => Ok(None),
            Some(r) => Ok(Some(
                JobRow {
                    id: r.0,
                    kind: r.1,
                    payload: r.2,
                    state: r.3,
                    attempts: r.4,
                    max_attempts: r.5,
                    run_at: r.6,
                    locked_until: r.7,
                    locked_by: r.8,
                    last_error: r.9,
                    created_at: r.10,
                    updated_at: r.11,
                }
                .try_into()?,
            )),
        }
    }
}

#[async_trait]
impl JobsEnqueuer for JobsRepositoryPg {
    async fn enqueue(&self, req: EnqueueRequest) -> anyhow::Result<Uuid> {
        JobsRepository::enqueue(self, req).await
    }
}
