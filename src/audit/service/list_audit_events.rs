use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::model::AuditEvent;
use crate::audit::persistence::{AuditCursor, AuditQueryFilter, AuditRepository};
use crate::auth::permissions::PermissionSet;
use crate::errors::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct ListAuditEventsRequest {
    pub organisation_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub category: Option<String>,
    pub outcome: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListAuditEventsResponse {
    pub items: Vec<AuditEvent>,
    pub next_cursor: Option<String>,
}

#[async_trait]
pub trait ListAuditEvents: Send + Sync + 'static {
    /// `caller_org` is the `org` from the caller's JWT; `perms` is their PermissionSet.
    /// Caller must hold `audit.read_own_org` or `audit.read_all`; this is enforced here.
    async fn execute(
        &self,
        req: ListAuditEventsRequest,
        caller_org: Uuid,
        perms: &PermissionSet,
    ) -> Result<ListAuditEventsResponse, AppError>;
}

pub struct ListAuditEventsImpl {
    repo: Arc<dyn AuditRepository>,
}

impl ListAuditEventsImpl {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self { Self { repo } }
}

#[async_trait]
impl ListAuditEvents for ListAuditEventsImpl {
    async fn execute(
        &self,
        req: ListAuditEventsRequest,
        caller_org: Uuid,
        perms: &PermissionSet,
    ) -> Result<ListAuditEventsResponse, AppError> {
        // Authorisation (spec §7.5):
        //   audit.read_all → any organisation_id (None ⇒ all orgs)
        //   audit.read_own_org → organisation_id must equal caller_org or be None (→ resolve to caller_org)
        let effective_org_filter: Option<Uuid> = if perms.is_audit_read_all() {
            req.organisation_id
        } else if perms.has("audit.read_own_org") {
            match req.organisation_id {
                None => Some(caller_org),
                Some(o) if o == caller_org => Some(o),
                Some(_) => return Err(AppError::NotFound { resource: "organisation".into() }),
            }
        } else {
            return Err(AppError::PermissionDenied { code: "audit.read_own_org".into() });
        };

        let limit = req.limit.unwrap_or(100).clamp(1, 200);
        let cursor = match req.cursor.as_deref() {
            Some(s) => Some(AuditCursor::decode(s).map_err(|_| AppError::Validation {
                errors: [("cursor".to_string(), vec!["invalid".to_string()])].into_iter().collect(),
            })?),
            None => None,
        };

        let filter = AuditQueryFilter {
            organisation_id: effective_org_filter,
            actor_user_id: req.actor_user_id,
            event_type: req.event_type,
            category: req.category,
            outcome: req.outcome,
            from: req.from,
            to: req.to,
            cursor,
            limit,
        };

        let page = self.repo.list_events(&filter).await.map_err(AppError::Internal)?;
        Ok(ListAuditEventsResponse {
            items: page.items,
            next_cursor: page.next_cursor.map(|c| c.encode()),
        })
    }
}
