-- Operator organisation (deterministic UUID, per spec §5.3)
INSERT INTO organisations (id, name, business, is_operator)
VALUES ('00000000-0000-0000-0000-000000000001', 'operator', 'Platform Operator', TRUE)
ON CONFLICT (id) DO NOTHING;

-- Built-in roles (deterministic UUIDs so tests can reference)
INSERT INTO roles (id, code, name, description, is_builtin) VALUES
  ('00000000-0000-0000-0000-000000000101', 'operator_admin', 'Operator Admin', 'Platform-wide administrator', TRUE),
  ('00000000-0000-0000-0000-000000000102', 'org_owner',      'Organisation Owner', 'Owns a tenant organisation', TRUE),
  ('00000000-0000-0000-0000-000000000103', 'org_admin',      'Organisation Admin', 'Manages a tenant organisation', TRUE),
  ('00000000-0000-0000-0000-000000000104', 'org_member',     'Organisation Member', 'Member of a tenant organisation', TRUE)
ON CONFLICT (id) DO NOTHING;

-- Permissions (UUIDv4-ish deterministic)
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000201', 'tenants.manage_all',      'Operate on any tenant, bypassing org scope'),
  ('00000000-0000-0000-0000-000000000202', 'users.manage_all',        'Manage any user account'),
  ('00000000-0000-0000-0000-000000000203', 'tenants.create',          'Create a new organisation'),
  ('00000000-0000-0000-0000-000000000204', 'tenants.update',          'Update an organisation the caller owns'),
  ('00000000-0000-0000-0000-000000000205', 'tenants.read',            'Read organisation metadata'),
  ('00000000-0000-0000-0000-000000000206', 'tenants.members.add',     'Add a user to an organisation'),
  ('00000000-0000-0000-0000-000000000207', 'tenants.members.remove',  'Remove a user from an organisation'),
  ('00000000-0000-0000-0000-000000000208', 'tenants.members.list',    'List members of an organisation'),
  ('00000000-0000-0000-0000-000000000209', 'tenants.roles.assign',    'Assign a role to a user in an organisation'),
  ('00000000-0000-0000-0000-00000000020a', 'audit.read_all',          'Read audit events across all organisations'),
  ('00000000-0000-0000-0000-00000000020b', 'audit.read_own_org',      'Read audit events for own organisation')
ON CONFLICT (id) DO NOTHING;

-- Role → permission mappings (spec §5.4)
-- operator_admin: everything
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'operator_admin'
  AND p.code IN (
    'tenants.manage_all', 'users.manage_all',
    'tenants.create', 'tenants.update', 'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_all', 'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_owner
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_owner'
  AND p.code IN (
    'tenants.create', 'tenants.update', 'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_admin
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_admin'
  AND p.code IN (
    'tenants.read',
    'tenants.members.add', 'tenants.members.remove', 'tenants.members.list',
    'tenants.roles.assign',
    'audit.read_own_org'
  )
ON CONFLICT DO NOTHING;

-- org_member
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.code = 'org_member'
  AND p.code IN (
    'tenants.read',
    'tenants.members.list'
  )
ON CONFLICT DO NOTHING;
