use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    Human,
    ServiceAccount,
}

impl UserKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UserKind::Human => "human",
            UserKind::ServiceAccount => "service_account",
        }
    }
}

impl std::str::FromStr for UserKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(UserKind::Human),
            "service_account" => Ok(UserKind::ServiceAccount),
            other => anyhow::bail!("unknown user kind: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub kind: UserKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub service_account_user_id: Uuid,
    pub prefix: String,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One-time response holding the plaintext key. Never persisted.
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyMaterial {
    pub key: ApiKey,
    pub plaintext: String,
}

#[derive(Debug, Clone)]
pub struct NewApiKey {
    pub service_account_user_id: Uuid,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub created_by: Uuid,
}
