use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One entry per org the user belongs to, enriched for the login response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMembership {
    pub org_id: Uuid,
    pub org_name: String,
    pub role_codes: Vec<String>,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PasswordResetToken {
    pub id: Uuid,
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
