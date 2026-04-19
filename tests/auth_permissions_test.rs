use egras::auth::permissions::PermissionSet;

#[test]
fn permission_set_matches_exact_code() {
    let s = PermissionSet::from_codes(vec!["tenants.read".into(), "tenants.members.list".into()]);
    assert!(s.has("tenants.read"));
    assert!(s.has("tenants.members.list"));
    assert!(!s.has("tenants.members.add"));
}

#[test]
fn permission_set_operator_flags() {
    let s = PermissionSet::from_codes(vec!["tenants.manage_all".into()]);
    assert!(s.is_operator_over_tenants());
    assert!(!s.is_audit_read_all());

    let s2 = PermissionSet::from_codes(vec!["audit.read_all".into()]);
    assert!(s2.is_audit_read_all());
}

#[test]
fn permission_set_any_match() {
    let s = PermissionSet::from_codes(vec!["tenants.members.add".into()]);
    assert!(s.has_any(&["users.manage_all", "tenants.members.add"]));
    assert!(!s.has_any(&["users.manage_all", "tenants.roles.assign"]));
}
