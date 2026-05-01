-- migrations/0008_inbound_channels.sql

CREATE TABLE inbound_channels (
    id               UUID        PRIMARY KEY,
    organisation_id  UUID        NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    name             TEXT        NOT NULL,
    description      TEXT,
    channel_type     TEXT        NOT NULL CHECK (channel_type IN ('vast', 'sensor', 'websocket', 'rest')),
    api_key          TEXT        NOT NULL,
    is_active        BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX inbound_channels_organisation_id_name_key
    ON inbound_channels (organisation_id, name);

-- New permission
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-00000000020c', 'channels.manage',
   'Manage inbound channels for an organisation')
ON CONFLICT (code) DO NOTHING;

-- operator_admin, org_owner, org_admin get channels.manage
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.code IN ('operator_admin', 'org_owner', 'org_admin')
  AND p.code = 'channels.manage'
ON CONFLICT DO NOTHING;
