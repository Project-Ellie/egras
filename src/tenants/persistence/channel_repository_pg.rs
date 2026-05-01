use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use sqlx::PgPool;
use uuid::Uuid;

use super::channel_repository::{ChannelRepoError, InboundChannelRepository};
use crate::tenants::model::{ChannelCursor, ChannelType, InboundChannel};

pub struct InboundChannelRepositoryPg {
    pool: PgPool,
}

impl InboundChannelRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn generate_api_key() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

fn map_insert_error(err: sqlx::Error, name: &str) -> ChannelRepoError {
    if let sqlx::Error::Database(ref dbe) = err {
        if dbe.code().as_deref() == Some("23505")
            && dbe
                .constraint()
                .map(|c| c.contains("organisation_id_name"))
                .unwrap_or(false)
        {
            return ChannelRepoError::DuplicateName(name.to_string());
        }
    }
    ChannelRepoError::Db(err)
}

#[async_trait]
impl InboundChannelRepository for InboundChannelRepositoryPg {
    async fn create(
        &self,
        organisation_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let id = Uuid::now_v7();
        let api_key = generate_api_key();
        let row = sqlx::query_as::<_, ChannelRow>(
            "INSERT INTO inbound_channels \
             (id, organisation_id, name, description, channel_type, api_key, is_active) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, organisation_id, name, description, channel_type, api_key, \
                       is_active, created_at, updated_at",
        )
        .bind(id)
        .bind(organisation_id)
        .bind(name)
        .bind(description)
        .bind(&channel_type)
        .bind(&api_key)
        .bind(is_active)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| map_insert_error(e, name))?;

        Ok(row.into())
    }

    async fn list(
        &self,
        organisation_id: Uuid,
        after: Option<ChannelCursor>,
        limit: u32,
    ) -> Result<Vec<InboundChannel>, ChannelRepoError> {
        let rows: Vec<ChannelRow> = if let Some(cursor) = after {
            sqlx::query_as::<_, ChannelRow>(
                "SELECT id, organisation_id, name, description, channel_type, api_key, \
                        is_active, created_at, updated_at \
                 FROM inbound_channels \
                 WHERE organisation_id = $1 \
                   AND (created_at, id) < ($2, $3) \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $4",
            )
            .bind(organisation_id)
            .bind(cursor.created_at)
            .bind(cursor.id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, ChannelRow>(
                "SELECT id, organisation_id, name, description, channel_type, api_key, \
                        is_active, created_at, updated_at \
                 FROM inbound_channels \
                 WHERE organisation_id = $1 \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $2",
            )
            .bind(organisation_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(InboundChannel::from).collect())
    }

    async fn get(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let row = sqlx::query_as::<_, ChannelRow>(
            "SELECT id, organisation_id, name, description, channel_type, api_key, \
                    is_active, created_at, updated_at \
             FROM inbound_channels \
             WHERE id = $1 AND organisation_id = $2",
        )
        .bind(channel_id)
        .bind(organisation_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ChannelRepoError::NotFound)?;

        Ok(row.into())
    }

    async fn update(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
        name: &str,
        description: Option<&str>,
        channel_type: ChannelType,
        is_active: bool,
    ) -> Result<InboundChannel, ChannelRepoError> {
        let row = sqlx::query_as::<_, ChannelRow>(
            "UPDATE inbound_channels \
             SET name = $1, description = $2, channel_type = $3, is_active = $4, \
                 updated_at = now() \
             WHERE id = $5 AND organisation_id = $6 \
             RETURNING id, organisation_id, name, description, channel_type, api_key, \
                       is_active, created_at, updated_at",
        )
        .bind(name)
        .bind(description)
        .bind(&channel_type)
        .bind(is_active)
        .bind(channel_id)
        .bind(organisation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| map_insert_error(e, name))?
        .ok_or(ChannelRepoError::NotFound)?;

        Ok(row.into())
    }

    async fn delete(
        &self,
        organisation_id: Uuid,
        channel_id: Uuid,
    ) -> Result<(), ChannelRepoError> {
        let result = sqlx::query(
            "DELETE FROM inbound_channels WHERE id = $1 AND organisation_id = $2",
        )
        .bind(channel_id)
        .bind(organisation_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ChannelRepoError::NotFound);
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct ChannelRow {
    id: Uuid,
    organisation_id: Uuid,
    name: String,
    description: Option<String>,
    channel_type: ChannelType,
    api_key: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ChannelRow> for InboundChannel {
    fn from(r: ChannelRow) -> Self {
        InboundChannel {
            id: r.id,
            organisation_id: r.organisation_id,
            name: r.name,
            description: r.description,
            channel_type: r.channel_type,
            api_key: r.api_key,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
