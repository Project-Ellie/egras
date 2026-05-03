-- 0011_service_accounts.sql
-- Service accounts (non-human principals) and per-SA API keys.

ALTER TABLE users
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'human'
    CHECK (kind IN ('human', 'service_account'));

CREATE TABLE service_accounts (
    user_id          UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    organisation_id  UUID NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    description      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by       UUID NOT NULL REFERENCES users(id),
    last_used_at     TIMESTAMPTZ,
    UNIQUE (organisation_id, name)
);

CREATE TABLE api_keys (
    id                       UUID PRIMARY KEY,
    service_account_user_id  UUID NOT NULL
        REFERENCES service_accounts(user_id) ON DELETE CASCADE,
    prefix                   TEXT NOT NULL UNIQUE,
    secret_hash              TEXT NOT NULL,
    name                     TEXT NOT NULL,
    scopes                   TEXT[],
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by               UUID NOT NULL REFERENCES users(id),
    last_used_at             TIMESTAMPTZ,
    revoked_at               TIMESTAMPTZ,
    CHECK (scopes IS NULL OR cardinality(scopes) > 0)
);

CREATE INDEX ix_api_keys_active_by_sa
    ON api_keys (service_account_user_id) WHERE revoked_at IS NULL;

INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000301', 'service_accounts.read',
      'List + read service accounts and API key metadata in own org'),
  ('00000000-0000-0000-0000-000000000302', 'service_accounts.manage',
      'Create / delete service accounts and API keys in own org')
ON CONFLICT (id) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code IN ('service_accounts.read', 'service_accounts.manage')
ON CONFLICT DO NOTHING;
