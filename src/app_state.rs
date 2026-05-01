use std::sync::Arc;

use crate::audit::service::{AuditRecorder, ListAuditEvents};
use crate::auth::jwt::JwtConfig;
use crate::security::persistence::{TokenRepository, UserRepository};
use crate::tenants::persistence::{OrganisationRepository, RoleRepository};

#[derive(Clone)]
pub struct AppState {
    pub audit_recorder: Arc<dyn AuditRecorder>,
    pub list_audit_events: Arc<dyn ListAuditEvents>,
    pub organisations: Arc<dyn OrganisationRepository>,
    pub roles: Arc<dyn RoleRepository>,
    pub users: Arc<dyn UserRepository>,
    pub tokens: Arc<dyn TokenRepository>,
    pub jwt_config: JwtConfig,
    pub password_reset_ttl_secs: i64,
}
