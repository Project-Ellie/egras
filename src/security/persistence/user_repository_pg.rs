use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::user_repository::{UserRepoError, UserRepository};
use crate::security::model::{User, UserMembership};

pub struct UserRepositoryPg {
    pool: PgPool,
}

impl UserRepositoryPg {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for UserRepositoryPg {
    async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<User, UserRepoError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, username, email, password_hash) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, username, email, password_hash, created_at, updated_at",
        )
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref dbe) = e {
                if dbe.code().as_deref() == Some("23505") {
                    if dbe.constraint() == Some("users_username_key") {
                        return UserRepoError::DuplicateUsername(username.to_string());
                    }
                    if dbe.constraint() == Some("users_email_key") {
                        return UserRepoError::DuplicateEmail(email.to_string());
                    }
                }
            }
            UserRepoError::Db(e)
        })?;
        Ok(row.into())
    }

    async fn find_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<User>, UserRepoError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, created_at, updated_at \
             FROM users WHERE username = $1 OR email = $1 LIMIT 1",
        )
        .bind(username_or_email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, UserRepoError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn update_password_hash(
        &self,
        user_id: Uuid,
        new_hash: &str,
    ) -> Result<(), UserRepoError> {
        sqlx::query(
            "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(new_hash)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_memberships(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserMembership>, UserRepoError> {
        let rows = sqlx::query_as::<_, MembershipRow>(
            "SELECT o.id AS org_id, o.name AS org_name, \
                    array_agg(DISTINCT r.code) AS role_codes, \
                    MIN(uor.created_at) AS joined_at \
             FROM user_organisation_roles uor \
             JOIN organisations o ON o.id = uor.organisation_id \
             JOIN roles r ON r.id = uor.role_id \
             WHERE uor.user_id = $1 \
             GROUP BY o.id, o.name \
             ORDER BY joined_at ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    email: String,
    password_hash: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            username: r.username,
            email: r.email,
            password_hash: r.password_hash,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MembershipRow {
    org_id: Uuid,
    org_name: String,
    role_codes: Vec<String>,
    joined_at: DateTime<Utc>,
}

impl From<MembershipRow> for UserMembership {
    fn from(r: MembershipRow) -> Self {
        UserMembership {
            org_id: r.org_id,
            org_name: r.org_name,
            role_codes: r.role_codes,
            joined_at: r.joined_at,
        }
    }
}
