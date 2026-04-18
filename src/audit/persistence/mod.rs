mod audit_repository_pg;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::model::AuditEvent;

pub use audit_repository_pg::AuditRepositoryPg;

/// Query filter for `list_events`. All fields optional except pagination.
#[derive(Debug, Clone, Default)]
pub struct AuditQueryFilter {
    pub organisation_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub category: Option<String>,
    pub outcome: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<AuditCursor>,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditCursor {
    pub occurred_at: DateTime<Utc>,
    pub id: Uuid,
}

impl AuditCursor {
    pub fn encode(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let json = serde_json::to_vec(self).expect("cursor serialises");
        URL_SAFE_NO_PAD.encode(json)
    }

    pub fn decode(s: &str) -> anyhow::Result<Self> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let bytes = URL_SAFE_NO_PAD.decode(s)?;
        let c: AuditCursor = serde_json::from_slice(&bytes)?;
        Ok(c)
    }
}

pub struct AuditQueryPage {
    pub items: Vec<AuditEvent>,
    pub next_cursor: Option<AuditCursor>,
}

#[async_trait]
pub trait AuditRepository: Send + Sync + 'static {
    async fn insert(&self, event: &AuditEvent) -> anyhow::Result<()>;
    async fn list_events(&self, filter: &AuditQueryFilter) -> anyhow::Result<AuditQueryPage>;
}
