-- 0014_grant_echo_to_org_roles.sql
-- Grant echo:invoke to org_admin and org_owner so service accounts can be
-- restricted to echo via per-key scopes (notebooks use this).
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code = 'echo:invoke'
ON CONFLICT DO NOTHING;
