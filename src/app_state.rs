use std::sync::Arc;

use sqlx::PgPool;

use crate::audit::service::{AuditRecorder, ListAuditEvents};
use crate::tenants::persistence::{OrganisationRepository, RoleRepository};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    pub organisations: Arc<dyn OrganisationRepository>,
    pub roles: Arc<dyn RoleRepository>,
}
