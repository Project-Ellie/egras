use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::feature_repository::{FeatureRepoError, FeatureRepository};
use crate::features::model::{FeatureDefinition, FeatureValueType, OrgFeatureOverride};

pub struct FeaturePgRepository {
    pool: PgPool,
}

impl FeaturePgRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ---------------------------------------------------------------------------
// Error mapping helpers
// ---------------------------------------------------------------------------

/// Maps a sqlx error, translating FK violation on `slug` → `UnknownSlug`.
fn map_slug_fk_error(e: sqlx::Error) -> FeatureRepoError {
    if let sqlx::Error::Database(ref dbe) = e {
        if dbe.code().as_deref() == Some("23503") {
            return FeatureRepoError::UnknownSlug;
        }
    }
    FeatureRepoError::Other(anyhow::Error::from(e))
}

// ---------------------------------------------------------------------------
// Row structs
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct DefinitionRow {
    slug: String,
    value_type: String,
    default_value: sqlx::types::Json<serde_json::Value>,
    description: String,
    self_service: bool,
}

impl TryFrom<DefinitionRow> for FeatureDefinition {
    type Error = FeatureRepoError;

    fn try_from(r: DefinitionRow) -> Result<Self, Self::Error> {
        let value_type = FeatureValueType::try_from_str(&r.value_type)
            .ok_or_else(|| anyhow::anyhow!("unknown value_type in DB: {}", r.value_type))
            .map_err(FeatureRepoError::Other)?;
        Ok(FeatureDefinition {
            slug: r.slug,
            value_type,
            default_value: r.default_value.0,
            description: r.description,
            self_service: r.self_service,
        })
    }
}

#[derive(sqlx::FromRow)]
struct OverrideRow {
    organisation_id: Uuid,
    slug: String,
    value: sqlx::types::Json<serde_json::Value>,
    updated_at: DateTime<Utc>,
    updated_by: Uuid,
}

impl From<OverrideRow> for OrgFeatureOverride {
    fn from(r: OverrideRow) -> Self {
        OrgFeatureOverride {
            organisation_id: r.organisation_id,
            slug: r.slug,
            value: r.value.0,
            updated_at: r.updated_at,
            updated_by: r.updated_by,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl FeatureRepository for FeaturePgRepository {
    async fn list_definitions(&self) -> Result<Vec<FeatureDefinition>, FeatureRepoError> {
        let rows = sqlx::query_as::<_, DefinitionRow>(
            "SELECT slug, value_type, default_value, description, self_service \
             FROM feature_definitions \
             ORDER BY slug ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FeatureRepoError::Other(anyhow::Error::from(e)))?;

        rows.into_iter()
            .map(FeatureDefinition::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn get_definition(
        &self,
        slug: &str,
    ) -> Result<Option<FeatureDefinition>, FeatureRepoError> {
        let row = sqlx::query_as::<_, DefinitionRow>(
            "SELECT slug, value_type, default_value, description, self_service \
             FROM feature_definitions \
             WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeatureRepoError::Other(anyhow::Error::from(e)))?;

        row.map(FeatureDefinition::try_from).transpose()
    }

    async fn list_overrides_for_org(
        &self,
        org: Uuid,
    ) -> Result<Vec<OrgFeatureOverride>, FeatureRepoError> {
        let rows = sqlx::query_as::<_, OverrideRow>(
            "SELECT organisation_id, slug, value, updated_at, updated_by \
             FROM organisation_features \
             WHERE organisation_id = $1 \
             ORDER BY slug ASC",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FeatureRepoError::Other(anyhow::Error::from(e)))?;

        Ok(rows.into_iter().map(OrgFeatureOverride::from).collect())
    }

    async fn get_override(
        &self,
        org: Uuid,
        slug: &str,
    ) -> Result<Option<OrgFeatureOverride>, FeatureRepoError> {
        let row = sqlx::query_as::<_, OverrideRow>(
            "SELECT organisation_id, slug, value, updated_at, updated_by \
             FROM organisation_features \
             WHERE organisation_id = $1 AND slug = $2",
        )
        .bind(org)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeatureRepoError::Other(anyhow::Error::from(e)))?;

        Ok(row.map(OrgFeatureOverride::from))
    }

    async fn upsert_override(
        &self,
        org: Uuid,
        slug: &str,
        value: serde_json::Value,
        updated_by: Uuid,
    ) -> Result<Option<serde_json::Value>, FeatureRepoError> {
        // CTE captures the old value before upsert, then the INSERT/ON CONFLICT
        // runs and returns it via a scalar sub-select.
        let old_value: Option<sqlx::types::Json<serde_json::Value>> = sqlx::query_scalar(
            r#"
            WITH prev AS (
                SELECT value
                FROM organisation_features
                WHERE organisation_id = $1 AND slug = $2
            )
            INSERT INTO organisation_features (organisation_id, slug, value, updated_by, updated_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (organisation_id, slug) DO UPDATE
                SET value      = EXCLUDED.value,
                    updated_by = EXCLUDED.updated_by,
                    updated_at = now()
            RETURNING (SELECT value FROM prev)
            "#,
        )
        .bind(org)
        .bind(slug)
        .bind(sqlx::types::Json(&value))
        .bind(updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(map_slug_fk_error)?;

        Ok(old_value.map(|j| j.0))
    }

    async fn delete_override(
        &self,
        org: Uuid,
        slug: &str,
    ) -> Result<Option<serde_json::Value>, FeatureRepoError> {
        let row: Option<sqlx::types::Json<serde_json::Value>> = sqlx::query_scalar(
            "DELETE FROM organisation_features \
             WHERE organisation_id = $1 AND slug = $2 \
             RETURNING value",
        )
        .bind(org)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeatureRepoError::Other(anyhow::Error::from(e)))?;

        Ok(row.map(|j| j.0))
    }
}
