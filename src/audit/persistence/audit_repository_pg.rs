use async_trait::async_trait;
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::PgPool;
use std::str::FromStr;

use super::{AuditCursor, AuditQueryFilter, AuditQueryPage, AuditRepository};
use crate::audit::model::{AuditCategory, AuditEvent, Outcome};

pub struct AuditRepositoryPg {
    pool: PgPool,
}

impl AuditRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditRepository for AuditRepositoryPg {
    async fn insert(&self, e: &AuditEvent) -> anyhow::Result<()> {
        let ip: Option<IpNetwork> = e
            .ip_address
            .as_deref()
            .and_then(|s| IpNetwork::from_str(s).ok());

        sqlx::query(
            r#"
            INSERT INTO audit_events
              (id, occurred_at, category, event_type,
               actor_user_id, actor_organisation_id,
               target_type, target_id, target_organisation_id,
               request_id, ip_address, user_agent,
               outcome, reason_code, payload)
            VALUES
              ($1, $2, $3, $4,
               $5, $6,
               $7, $8, $9,
               $10, $11, $12,
               $13, $14, $15)
            "#,
        )
        .bind(e.id)
        .bind(e.occurred_at)
        .bind(e.category.as_str())
        .bind(&e.event_type)
        .bind(e.actor_user_id)
        .bind(e.actor_organisation_id)
        .bind(&e.target_type)
        .bind(e.target_id)
        .bind(e.target_organisation_id)
        .bind(&e.request_id)
        .bind(ip)
        .bind(&e.user_agent)
        .bind(e.outcome.as_str())
        .bind(&e.reason_code)
        .bind(&e.payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_events(&self, f: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage> {
        // Build query with dynamic filters. Parameters are bound positionally; we
        // use a QueryBuilder to keep things readable and injection-safe.
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, occurred_at, category, event_type, \
                    actor_user_id, actor_organisation_id, \
                    target_type, target_id, target_organisation_id, \
                    request_id, host(ip_address) AS ip_address, user_agent, \
                    outcome, reason_code, payload \
             FROM audit_events WHERE 1=1 ",
        );

        if let Some(org) = f.organisation_id {
            qb.push(" AND target_organisation_id = ");
            qb.push_bind(org);
        }
        if let Some(actor) = f.actor_user_id {
            qb.push(" AND actor_user_id = ");
            qb.push_bind(actor);
        }
        if let Some(ref et) = f.event_type {
            qb.push(" AND event_type = ");
            qb.push_bind(et);
        }
        if let Some(ref cat) = f.category {
            qb.push(" AND category = ");
            qb.push_bind(cat);
        }
        if let Some(ref out) = f.outcome {
            qb.push(" AND outcome = ");
            qb.push_bind(out);
        }
        if let Some(from) = f.from {
            qb.push(" AND occurred_at >= ");
            qb.push_bind(from);
        }
        if let Some(to) = f.to {
            qb.push(" AND occurred_at <= ");
            qb.push_bind(to);
        }
        if let Some(ref c) = f.cursor {
            qb.push(" AND (occurred_at, id) < (");
            qb.push_bind(c.occurred_at)
                .push(", ")
                .push_bind(c.id)
                .push(")");
        }

        qb.push(" ORDER BY occurred_at DESC, id DESC LIMIT ");
        qb.push_bind(f.limit + 1); // fetch one extra to determine next_cursor

        let rows = qb.build().fetch_all(&self.pool).await?;

        let mut items: Vec<AuditEvent> = Vec::with_capacity(rows.len().min(f.limit as usize));
        for row in rows.iter().take(f.limit as usize) {
            use sqlx::Row;
            let category_str: String = row.try_get("category")?;
            let outcome_str: String = row.try_get("outcome")?;
            items.push(AuditEvent {
                id: row.try_get("id")?,
                occurred_at: row.try_get("occurred_at")?,
                category: AuditCategory::try_from_str(&category_str)
                    .ok_or_else(|| anyhow::anyhow!("unknown category: {category_str}"))?,
                event_type: row.try_get("event_type")?,
                actor_user_id: row.try_get("actor_user_id")?,
                actor_organisation_id: row.try_get("actor_organisation_id")?,
                target_type: row.try_get("target_type")?,
                target_id: row.try_get("target_id")?,
                target_organisation_id: row.try_get("target_organisation_id")?,
                request_id: row.try_get("request_id")?,
                ip_address: row.try_get::<Option<String>, _>("ip_address")?,
                user_agent: row.try_get("user_agent")?,
                outcome: Outcome::try_from_str(&outcome_str)
                    .ok_or_else(|| anyhow::anyhow!("unknown outcome: {outcome_str}"))?,
                reason_code: row.try_get("reason_code")?,
                payload: row.try_get("payload")?,
            });
        }

        let next_cursor = if rows.len() as i64 > f.limit {
            items.last().map(|last| AuditCursor {
                occurred_at: last.occurred_at,
                id: last.id,
            })
        } else {
            None
        };

        Ok(AuditQueryPage { items, next_cursor })
    }
}
