use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organisation {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub is_operator: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: Uuid,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub user_id: Uuid,
    pub organisation_id: Uuid,
    pub role_id: Uuid,
    pub role_code: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganisationSummary {
    pub id: Uuid,
    pub name: String,
    pub business: String,
    pub role_codes: Vec<String>,
    /// Carried through so the caller can build `OrganisationCursor` from a page row.
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_codes: Vec<String>,
    /// Earliest join timestamp for this user in the organisation. Used to build `MembershipCursor`.
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganisationCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipCursor {
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
}
