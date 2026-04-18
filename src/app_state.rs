use std::sync::Arc;

use sqlx::PgPool;

use crate::audit::service::{AuditRecorder, ListAuditEvents};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    // Plan 2 will add: register_user, login, change_password, switch_org,
    //                   create_organisation, add_user_to_organisation, etc.
}
