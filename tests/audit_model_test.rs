use egras::audit::model::{AuditCategory, AuditEvent, Outcome};

#[test]
fn category_as_str_matches_db_values() {
    assert_eq!(AuditCategory::SecurityStateChange.as_str(), "security.state_change");
    assert_eq!(AuditCategory::SecurityAuth.as_str(),         "security.auth");
    assert_eq!(AuditCategory::SecurityPermissionDenial.as_str(), "security.permission_denial");
    assert_eq!(AuditCategory::TenantsStateChange.as_str(),  "tenants.state_change");
}

#[test]
fn outcome_as_str() {
    assert_eq!(Outcome::Success.as_str(), "success");
    assert_eq!(Outcome::Failure.as_str(), "failure");
    assert_eq!(Outcome::Denied.as_str(),  "denied");
}

#[test]
fn user_registered_event_shape() {
    let e = AuditEvent::user_registered_success(
        uuid::Uuid::now_v7(),  // actor user
        uuid::Uuid::now_v7(),  // actor org
        uuid::Uuid::now_v7(),  // target user
        uuid::Uuid::now_v7(),  // target org
        "org_member".into(),
    );
    assert_eq!(e.category, AuditCategory::SecurityStateChange);
    assert_eq!(e.event_type, "user.registered");
    assert_eq!(e.outcome, Outcome::Success);
    assert_eq!(e.target_type.as_deref(), Some("user"));
    assert_eq!(e.payload["role_code"], "org_member");
}
