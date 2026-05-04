use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditCategory {
    SecurityStateChange,
    SecurityAuth,
    SecurityPermissionDenial,
    DataAccess,
    TenantsStateChange,
}

impl AuditCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SecurityStateChange => "security.state_change",
            Self::SecurityAuth => "security.auth",
            Self::SecurityPermissionDenial => "security.permission_denial",
            Self::DataAccess => "data.access",
            Self::TenantsStateChange => "tenants.state_change",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "security.state_change" => Self::SecurityStateChange,
            "security.auth" => Self::SecurityAuth,
            "security.permission_denial" => Self::SecurityPermissionDenial,
            "data.access" => Self::DataAccess,
            "tenants.state_change" => Self::TenantsStateChange,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Success,
    Failure,
    Denied,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Denied => "denied",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "success" => Self::Success,
            "failure" => Self::Failure,
            "denied" => Self::Denied,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub user_id: Option<Uuid>,
    pub organisation_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl Actor {
    pub fn system() -> Self {
        Self {
            user_id: None,
            organisation_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
        }
    }
}

/// An audit event ready to be recorded. Use `AuditEvent::*` constructors to build these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub category: AuditCategory,
    pub event_type: String,
    pub actor_user_id: Option<Uuid>,
    pub actor_organisation_id: Option<Uuid>,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub target_organisation_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub outcome: Outcome,
    pub reason_code: Option<String>,
    pub payload: Value,
}

impl AuditEvent {
    fn base(category: AuditCategory, event_type: &str, outcome: Outcome) -> Self {
        Self {
            id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            category,
            event_type: event_type.to_string(),
            actor_user_id: None,
            actor_organisation_id: None,
            target_type: None,
            target_id: None,
            target_organisation_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            outcome,
            reason_code: None,
            payload: json!({}),
        }
    }

    pub fn with_actor(mut self, actor: &Actor) -> Self {
        self.actor_user_id = actor.user_id;
        self.actor_organisation_id = actor.organisation_id;
        self.request_id = actor.request_id.clone();
        self.ip_address = actor.ip_address.clone();
        self.user_agent = actor.user_agent.clone();
        self
    }

    pub fn user_registered_success(
        actor_user: Uuid,
        actor_org: Uuid,
        target_user: Uuid,
        target_org: Uuid,
        role_code: String,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "user.registered",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor_user);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(target_user);
        e.target_organisation_id = Some(target_org);
        e.payload = json!({ "role_code": role_code });
        e
    }

    pub fn login_success(user_id: Uuid, active_org: Uuid) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityAuth,
            "login.success",
            Outcome::Success,
        );
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(active_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e
    }

    pub fn login_failed(reason_code: &str, username_or_email: &str) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityAuth,
            "login.failed",
            Outcome::Failure,
        );
        e.reason_code = Some(reason_code.into());
        e.payload = json!({ "username_or_email": username_or_email });
        e
    }

    pub fn logout(user_id: Uuid, org: Uuid, jti: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::SecurityAuth, "logout", Outcome::Success);
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(org);
        e.payload = json!({ "jti": jti });
        e
    }

    pub fn session_switched_org(user_id: Uuid, from_org: Uuid, to_org: Uuid) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityAuth,
            "session.switched_org",
            Outcome::Success,
        );
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(to_org);
        e.target_type = Some("organisation".into());
        e.target_id = Some(to_org);
        e.target_organisation_id = Some(to_org);
        e.payload = json!({ "from_org": from_org });
        e
    }

    pub fn password_changed(user_id: Uuid) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "password.changed",
            Outcome::Success,
        );
        e.actor_user_id = Some(user_id);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e
    }

    pub fn password_reset_requested(email: &str) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "password.reset_requested",
            Outcome::Success,
        );
        e.payload = json!({ "email": email });
        e
    }

    pub fn password_reset_confirmed(
        user_id: Option<Uuid>,
        outcome: Outcome,
        reason: Option<String>,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "password.reset_confirmed",
            outcome,
        );
        e.actor_user_id = user_id;
        e.reason_code = reason;
        e
    }

    pub fn permission_denied(user_id: Uuid, org: Uuid, permission: &str, path: &str) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityPermissionDenial,
            "permission.denied",
            Outcome::Denied,
        );
        e.actor_user_id = Some(user_id);
        e.actor_organisation_id = Some(org);
        e.reason_code = Some(format!("missing:{permission}"));
        e.payload = json!({ "path": path });
        e
    }

    pub fn organisation_created(actor: Uuid, actor_org: Uuid, org_id: Uuid, name: &str) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "organisation.created",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("organisation".into());
        e.target_id = Some(org_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "name": name });
        e
    }

    pub fn organisation_member_added(
        actor: Uuid,
        actor_org: Uuid,
        org_id: Uuid,
        user_id: Uuid,
        role_code: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "organisation.member_added",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "role_code": role_code });
        e
    }

    pub fn organisation_member_removed(
        actor: Uuid,
        actor_org: Uuid,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "organisation.member_removed",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e
    }

    pub fn organisation_role_assigned(
        actor: Uuid,
        actor_org: Uuid,
        org_id: Uuid,
        user_id: Uuid,
        role_code: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "organisation.role_assigned",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "role_code": role_code });
        e
    }

    pub fn users_list(actor_user_id: Uuid, actor_org_id: Uuid) -> Self {
        let mut e = Self::base(AuditCategory::DataAccess, "users.list", Outcome::Success);
        e.actor_user_id = Some(actor_user_id);
        e.actor_organisation_id = Some(actor_org_id);
        e.target_type = Some("user".into());
        e
    }

    pub fn admin_seeded(user_id: Uuid, org_id: Uuid, role_code: &str) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "user.registered",
            Outcome::Success,
        );
        // actor is None — this is a system/CLI operation, not a user request
        e.target_type = Some("user".into());
        e.target_id = Some(user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = serde_json::json!({ "role_code": role_code, "via": "seed-admin" });
        e
    }

    pub fn channel_created(
        actor: Uuid,
        actor_org: Uuid,
        channel_id: Uuid,
        org_id: Uuid,
        name: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "channel.created",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("inbound_channel".into());
        e.target_id = Some(channel_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "name": name });
        e
    }

    pub fn channel_updated(
        actor: Uuid,
        actor_org: Uuid,
        channel_id: Uuid,
        org_id: Uuid,
        name: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "channel.updated",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("inbound_channel".into());
        e.target_id = Some(channel_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "name": name });
        e
    }

    pub fn channel_deleted(actor: Uuid, actor_org: Uuid, channel_id: Uuid, org_id: Uuid) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "channel.deleted",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("inbound_channel".into());
        e.target_id = Some(channel_id);
        e.target_organisation_id = Some(org_id);
        e
    }

    pub fn service_account_created(
        actor: Uuid,
        actor_org: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
        name: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "service_account.created",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("service_account".into());
        e.target_id = Some(sa_user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "name": name });
        e
    }

    pub fn service_account_deleted(
        actor: Uuid,
        actor_org: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "service_account.deleted",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("service_account".into());
        e.target_id = Some(sa_user_id);
        e.target_organisation_id = Some(org_id);
        e
    }

    pub fn api_key_created(
        actor: Uuid,
        actor_org: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
        key_id: Uuid,
        prefix: &str,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "api_key.created",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("api_key".into());
        e.target_id = Some(sa_user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "key_id": key_id, "prefix": prefix });
        e
    }

    pub fn api_key_revoked(
        actor: Uuid,
        actor_org: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
        key_id: Uuid,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "api_key.revoked",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("api_key".into());
        e.target_id = Some(sa_user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "key_id": key_id });
        e
    }

    pub fn feature_set(
        actor: Uuid,
        actor_org: Uuid,
        org_id: Uuid,
        slug: &str,
        old_value: Option<&Value>,
        new_value: &Value,
        self_service: bool,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "feature.set",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("feature".into());
        e.target_organisation_id = Some(org_id);
        e.payload = json!({
            "slug": slug,
            "old_value": old_value,
            "new_value": new_value,
            "self_service": self_service,
        });
        e
    }

    pub fn feature_cleared(
        actor: Uuid,
        actor_org: Uuid,
        org_id: Uuid,
        slug: &str,
        old_value: Option<&Value>,
        self_service: bool,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::TenantsStateChange,
            "feature.cleared",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("feature".into());
        e.target_organisation_id = Some(org_id);
        e.payload = json!({
            "slug": slug,
            "old_value": old_value,
            "self_service": self_service,
        });
        e
    }

    pub fn api_key_rotated(
        actor: Uuid,
        actor_org: Uuid,
        sa_user_id: Uuid,
        org_id: Uuid,
        old_key_id: Uuid,
        new_key_id: Uuid,
    ) -> Self {
        let mut e = Self::base(
            AuditCategory::SecurityStateChange,
            "api_key.rotated",
            Outcome::Success,
        );
        e.actor_user_id = Some(actor);
        e.actor_organisation_id = Some(actor_org);
        e.target_type = Some("api_key".into());
        e.target_id = Some(sa_user_id);
        e.target_organisation_id = Some(org_id);
        e.payload = json!({ "old_key_id": old_key_id, "new_key_id": new_key_id });
        e
    }
}
