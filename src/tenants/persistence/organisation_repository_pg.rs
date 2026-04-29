use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::organisation_repository::{OrganisationRepository, RepoError};
use crate::tenants::model::{
    MemberSummary, MembershipCursor, Organisation, OrganisationCursor, OrganisationSummary,
};

pub struct OrganisationRepositoryPg {
    pool: PgPool,
}

impl OrganisationRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Maps a sqlx error from an INSERT into `organisations`, translating the unique
/// constraint violation on `name` to `RepoError::DuplicateName`.
fn map_org_insert_error(err: sqlx::Error, name: &str) -> RepoError {
    if let sqlx::Error::Database(ref dbe) = err {
        if dbe.code().as_deref() == Some("23505")
            && dbe.constraint() == Some("organisations_name_key")
        {
            return RepoError::DuplicateName(name.to_string());
        }
    }
    RepoError::Db(err)
}

#[async_trait]
impl OrganisationRepository for OrganisationRepositoryPg {
    async fn create(&self, name: &str, business: &str) -> Result<Organisation, RepoError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, OrgRow>(
            "INSERT INTO organisations (id, name, business, is_operator) \
             VALUES ($1, $2, $3, FALSE) \
             RETURNING id, name, business, is_operator, created_at, updated_at",
        )
        .bind(id)
        .bind(name)
        .bind(business)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| map_org_insert_error(e, name))?;

        Ok(row.into())
    }

    async fn create_with_initial_owner(
        &self,
        name: &str,
        business: &str,
        creator_user_id: Uuid,
        owner_role_code: &str,
    ) -> Result<Organisation, RepoError> {
        let mut tx = self.pool.begin().await?;

        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, OrgRow>(
            "INSERT INTO organisations (id, name, business, is_operator) \
             VALUES ($1, $2, $3, FALSE) \
             RETURNING id, name, business, is_operator, created_at, updated_at",
        )
        .bind(id)
        .bind(name)
        .bind(business)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_org_insert_error(e, name))?;

        let org = Organisation::from(row);

        // Resolve the role by code.
        let role_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
            .bind(owner_role_code)
            .fetch_optional(&mut *tx)
            .await?;

        let role_id =
            role_id.ok_or_else(|| RepoError::UnknownRoleCode(owner_role_code.to_string()))?;

        // Insert the membership row.
        sqlx::query(
            "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
             VALUES ($1, $2, $3)",
        )
        .bind(creator_user_id)
        .bind(org.id)
        .bind(role_id)
        .execute(&mut *tx)
        .await
        // Only the user FK can fail in practice: the organisation was just
        // inserted in this same transaction, and the role_id was resolved from
        // a successful SELECT above. Any other FK failure indicates schema
        // drift and is surfaced as `Db(err)`.
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23503")
                    && dbe.constraint() == Some("user_organisation_roles_user_id_fkey")
                {
                    return RepoError::UnknownUser(creator_user_id);
                }
            }
            RepoError::Db(e)
        })?;

        tx.commit().await?;
        Ok(org)
    }

    async fn list_for_user(
        &self,
        user_id: Uuid,
        after: Option<OrganisationCursor>,
        limit: u32,
    ) -> Result<Vec<OrganisationSummary>, RepoError> {
        let rows: Vec<OrgSummaryRow> = if let Some(cursor) = after {
            sqlx::query_as::<_, OrgSummaryRow>(
                "SELECT o.id, o.name, o.business, o.created_at, \
                        array_agg(DISTINCT r.code) AS role_codes \
                 FROM organisations o \
                 JOIN user_organisation_roles uor ON uor.organisation_id = o.id \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.user_id = $1 \
                   AND (o.created_at, o.id) < ($2, $3) \
                 GROUP BY o.id \
                 ORDER BY o.created_at DESC, o.id DESC \
                 LIMIT $4",
            )
            .bind(user_id)
            .bind(cursor.created_at)
            .bind(cursor.id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, OrgSummaryRow>(
                "SELECT o.id, o.name, o.business, o.created_at, \
                        array_agg(DISTINCT r.code) AS role_codes \
                 FROM organisations o \
                 JOIN user_organisation_roles uor ON uor.organisation_id = o.id \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.user_id = $1 \
                 GROUP BY o.id \
                 ORDER BY o.created_at DESC, o.id DESC \
                 LIMIT $2",
            )
            .bind(user_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(OrganisationSummary::from).collect())
    }

    async fn list_members(
        &self,
        organisation_id: Uuid,
        after: Option<MembershipCursor>,
        limit: u32,
    ) -> Result<Vec<MemberSummary>, RepoError> {
        let rows: Vec<MemberRow> = if let Some(cursor) = after {
            sqlx::query_as::<_, MemberRow>(
                "SELECT u.id AS user_id, u.username, u.email, \
                        array_agg(DISTINCT r.code) AS role_codes, \
                        MIN(uor.created_at) AS joined_at \
                 FROM user_organisation_roles uor \
                 JOIN users u ON u.id = uor.user_id \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.organisation_id = $1 \
                 GROUP BY u.id, u.username, u.email \
                 HAVING (MIN(uor.created_at), u.id) < ($2, $3) \
                 ORDER BY joined_at DESC, u.id DESC \
                 LIMIT $4",
            )
            .bind(organisation_id)
            .bind(cursor.created_at)
            .bind(cursor.user_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, MemberRow>(
                "SELECT u.id AS user_id, u.username, u.email, \
                        array_agg(DISTINCT r.code) AS role_codes, \
                        MIN(uor.created_at) AS joined_at \
                 FROM user_organisation_roles uor \
                 JOIN users u ON u.id = uor.user_id \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.organisation_id = $1 \
                 GROUP BY u.id, u.username, u.email \
                 ORDER BY joined_at DESC, u.id DESC \
                 LIMIT $2",
            )
            .bind(organisation_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(MemberSummary::from).collect())
    }

    async fn is_member(&self, user_id: Uuid, organisation_id: Uuid) -> Result<bool, RepoError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                SELECT 1 FROM user_organisation_roles \
                WHERE user_id = $1 AND organisation_id = $2\
             )",
        )
        .bind(user_id)
        .bind(organisation_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    async fn add_member(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        role_code: &str,
    ) -> Result<(), RepoError> {
        let role_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
            .bind(role_code)
            .fetch_optional(&self.pool)
            .await?;

        let role_id = role_id.ok_or_else(|| RepoError::UnknownRoleCode(role_code.to_string()))?;

        sqlx::query(
            "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(org_id)
        .bind(role_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23503") {
                    return RepoError::NotFound;
                }
            }
            RepoError::Db(e)
        })?;

        Ok(())
    }

    async fn remove_member_checked(&self, user_id: Uuid, org_id: Uuid) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await?;

        // Lock and check membership.
        let is_member: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM user_organisation_roles \
                 WHERE user_id = $1 AND organisation_id = $2 \
             )",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;

        if !is_member {
            return Err(RepoError::NotMember);
        }

        // Count org_owner rows for other users (exclusive lock via FOR UPDATE).
        let other_owners: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) \
             FROM user_organisation_roles uor \
             JOIN roles r ON r.id = uor.role_id \
             WHERE uor.organisation_id = $1 \
               AND r.code = 'org_owner' \
               AND uor.user_id != $2 \
             FOR UPDATE",
        )
        .bind(org_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        // Check if target holds org_owner.
        let target_is_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM user_organisation_roles uor \
                 JOIN roles r ON r.id = uor.role_id \
                 WHERE uor.user_id = $1 AND uor.organisation_id = $2 \
                   AND r.code = 'org_owner' \
             )",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;

        if target_is_owner && other_owners == 0 {
            return Err(RepoError::LastOwner);
        }

        sqlx::query(
            "DELETE FROM user_organisation_roles \
             WHERE user_id = $1 AND organisation_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal row structs for FromRow mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct OrgRow {
    id: Uuid,
    name: String,
    business: String,
    is_operator: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OrgRow> for Organisation {
    fn from(r: OrgRow) -> Self {
        Organisation {
            id: r.id,
            name: r.name,
            business: r.business,
            is_operator: r.is_operator,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct OrgSummaryRow {
    id: Uuid,
    name: String,
    business: String,
    created_at: DateTime<Utc>,
    role_codes: Vec<String>,
}

impl From<OrgSummaryRow> for OrganisationSummary {
    fn from(r: OrgSummaryRow) -> Self {
        OrganisationSummary {
            id: r.id,
            name: r.name,
            business: r.business,
            role_codes: r.role_codes,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    user_id: Uuid,
    username: String,
    email: String,
    role_codes: Vec<String>,
    joined_at: DateTime<Utc>,
}

impl From<MemberRow> for MemberSummary {
    fn from(r: MemberRow) -> Self {
        MemberSummary {
            user_id: r.user_id,
            username: r.username,
            email: r.email,
            role_codes: r.role_codes,
            joined_at: r.joined_at,
        }
    }
}
