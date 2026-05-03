use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub aggregate_type: Option<String>,
    pub aggregate_id: Option<Uuid>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub relayed_at: Option<DateTime<Utc>>,
    pub relay_attempts: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppendRequest {
    pub aggregate_type: Option<String>,
    pub aggregate_id: Option<Uuid>,
    pub event_type: String,
    pub payload: serde_json::Value,
}

impl AppendRequest {
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            aggregate_type: None,
            aggregate_id: None,
            event_type: event_type.into(),
            payload,
        }
    }

    pub fn with_aggregate(mut self, aggregate_type: impl Into<String>, id: Uuid) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self.aggregate_id = Some(id);
        self
    }
}
