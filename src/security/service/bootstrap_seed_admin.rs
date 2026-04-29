use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditRepository, AuditRepositoryPg};
use crate::security::service::password_hash::hash_password;

#[derive(Debug, Clone)]
pub struct SeedAdminInput {
    pub email: String,
    pub username: String,
    pub password: String,
    pub role_code: String,
    pub operator_org_name: String,
}

#[derive(Debug, Clone)]
pub struct SeedAdminOutput {
    pub user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum SeedAdminError {
    #[error("operator organisation '{0}' does not exist — run migrations first")]
    OperatorOrgNotFound(String),
    #[error("user with email '{0}' already exists — refusing to overwrite")]
    UserAlreadyExists(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn bootstrap_seed_admin(
    pool: &PgPool,
    input: SeedAdminInput,
) -> Result<SeedAdminOutput, SeedAdminError> {
    // 1. Find the operator org.
    let org_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM organisations WHERE name = $1")
        .bind(&input.operator_org_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| SeedAdminError::Internal(e.into()))?;

    let org_id = org_id
        .ok_or_else(|| SeedAdminError::OperatorOrgNotFound(input.operator_org_name.clone()))?;

    // 2. Refuse if the email already exists.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
        .bind(&input.email)
        .fetch_one(pool)
        .await
        .map_err(|e| SeedAdminError::Internal(e.into()))?;

    if exists {
        return Err(SeedAdminError::UserAlreadyExists(input.email.clone()));
    }

    // 3. Hash password.
    let password_hash = hash_password(&input.password).map_err(SeedAdminError::Internal)?;

    // 4. Insert user.
    let user_id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(&input.username)
        .bind(&input.email)
        .bind(&password_hash)
        .execute(pool)
        .await
        .map_err(|e| SeedAdminError::Internal(e.into()))?;

    // 5. Resolve role_id and insert membership.
    let role_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
        .bind(&input.role_code)
        .fetch_optional(pool)
        .await
        .map_err(|e| SeedAdminError::Internal(e.into()))?;

    let role_id = role_id.ok_or_else(|| {
        SeedAdminError::Internal(anyhow::anyhow!(
            "role '{}' not found in database",
            input.role_code
        ))
    })?;

    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id)
         VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(role_id)
    .execute(pool)
    .await
    .map_err(|e| SeedAdminError::Internal(e.into()))?;

    // 6. Write audit row synchronously (no worker channel in CLI context).
    let event = AuditEvent::admin_seeded(user_id, org_id, &input.role_code);
    AuditRepositoryPg::new(pool.clone())
        .insert(&event)
        .await
        .map_err(SeedAdminError::Internal)?;

    Ok(SeedAdminOutput { user_id })
}
