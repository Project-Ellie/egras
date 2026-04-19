use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use super::organisation_repository::RepoError;
use super::role_repository::RoleRepository;
use crate::tenants::model::Role;

pub struct RoleRepositoryPg {
    pool: PgPool,
}

impl RoleRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct RoleRow {
    id: Uuid,
    code: String,
}

#[async_trait]
impl RoleRepository for RoleRepositoryPg {
    async fn find_by_code(&self, code: &str) -> Result<Option<Role>, RepoError> {
        let row = sqlx::query_as::<_, RoleRow>("SELECT id, code FROM roles WHERE code = $1")
            .bind(code)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| Role {
            id: r.id,
            code: r.code,
        }))
    }

    async fn assign(
        &self,
        user_id: Uuid,
        organisation_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(organisation_id)
        .bind(role_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23503") {
                    match dbe.constraint() {
                        Some("user_organisation_roles_user_id_fkey") => {
                            return RepoError::UnknownUser(user_id);
                        }
                        Some("user_organisation_roles_role_id_fkey") => {
                            return RepoError::Db(e);
                        }
                        _ => {}
                    }
                }
            }
            RepoError::Db(e)
        })?;

        Ok(())
    }
}
