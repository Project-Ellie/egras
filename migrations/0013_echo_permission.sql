-- 0013_echo_permission.sql
-- echo:invoke permission. Service accounts may receive this via per-key
-- scopes at mint time, or by adding it to a service-account-bearing role.
-- Not granted to any role by default.

INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000501', 'echo:invoke',
   'Invoke the /api/v1/echo endpoint (smoke-test target).')
ON CONFLICT (id) DO NOTHING;
