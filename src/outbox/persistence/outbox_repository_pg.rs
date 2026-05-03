use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::OutboxRepository;
use crate::outbox::model::{AppendRequest, OutboxEvent};
use crate::outbox::relayer::OutboxAppender;

pub struct OutboxRepositoryPg {
    pool: PgPool,
}

impl OutboxRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

type OutboxRow = (
    Uuid,
    Option<String>,
    Option<Uuid>,
    String,
    serde_json::Value,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    i32,
    Option<String>,
);

fn row_to_event(r: OutboxRow) -> OutboxEvent {
    OutboxEvent {
        id: r.0,
        aggregate_type: r.1,
        aggregate_id: r.2,
        event_type: r.3,
        payload: r.4,
        created_at: r.5,
        relayed_at: r.6,
        relay_attempts: r.7,
        last_error: r.8,
    }
}

#[async_trait]
impl OutboxRepository for OutboxRepositoryPg {
    async fn append_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        req: AppendRequest,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO outbox_events
              (id, aggregate_type, aggregate_id, event_type, payload)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(&req.aggregate_type)
        .bind(req.aggregate_id)
        .bind(&req.event_type)
        .bind(&req.payload)
        .execute(&mut *tx)
        .await?;
        Ok(id)
    }

    async fn claim_unrelayed_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        limit: u32,
    ) -> anyhow::Result<Vec<OutboxEvent>> {
        let rows = sqlx::query_as::<_, OutboxRow>(
            r#"
            SELECT id, aggregate_type, aggregate_id, event_type, payload,
                   created_at, relayed_at, relay_attempts, last_error
              FROM outbox_events
             WHERE relayed_at IS NULL
             ORDER BY created_at ASC, id ASC
             LIMIT $1
             FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await?;
        Ok(rows.into_iter().map(row_to_event).collect())
    }

    async fn mark_relayed_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        ids: &[Uuid],
    ) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            r#"
            UPDATE outbox_events
               SET relayed_at = now()
             WHERE id = ANY($1)
            "#,
        )
        .bind(ids)
        .execute(&mut *tx)
        .await?;
        Ok(())
    }

    async fn find(&self, id: Uuid) -> anyhow::Result<Option<OutboxEvent>> {
        let row = sqlx::query_as::<_, OutboxRow>(
            r#"
            SELECT id, aggregate_type, aggregate_id, event_type, payload,
                   created_at, relayed_at, relay_attempts, last_error
              FROM outbox_events
             WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_event))
    }
}

#[async_trait]
impl OutboxAppender for OutboxRepositoryPg {
    async fn append_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        req: AppendRequest,
    ) -> anyhow::Result<Uuid> {
        OutboxRepository::append_in_tx(self, tx, req).await
    }
}

impl OutboxRepositoryPg {
    /// Convenience for tests / non-tx-coupled callers.
    pub async fn append_standalone(&self, req: AppendRequest) -> anyhow::Result<Uuid> {
        let mut tx = self.pool.begin().await?;
        let id = OutboxRepository::append_in_tx(self, &mut tx, req).await?;
        tx.commit().await?;
        Ok(id)
    }
}
