-- 0012_features.sql
-- Per-organisation feature flags: catalog (seeded here) + sparse overrides per org.

-- Catalog: authoritative list of flags, seeded via migrations. App code references slugs as constants.
CREATE TABLE feature_definitions (
    slug          TEXT PRIMARY KEY,
    value_type    TEXT NOT NULL CHECK (value_type IN ('bool','string','int','enum_set','json')),
    default_value JSONB NOT NULL,
    description   TEXT NOT NULL,
    self_service  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sparse per-org overrides.
CREATE TABLE organisation_features (
    organisation_id UUID NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL REFERENCES feature_definitions(slug) ON DELETE RESTRICT,
    value           JSONB NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID NOT NULL REFERENCES users(id),
    PRIMARY KEY (organisation_id, slug)
);

CREATE INDEX ix_org_features_by_org ON organisation_features (organisation_id);

-- Permissions
INSERT INTO permissions (id, code, description) VALUES
  ('00000000-0000-0000-0000-000000000401', 'features.read',
      'Read feature flag values for own organisation'),
  ('00000000-0000-0000-0000-000000000402', 'features.manage',
      'Set feature flag overrides for own organisation (self_service flags only; operators bypass)')
ON CONFLICT (id) DO NOTHING;

-- features.read for org_admin / org_owner only (not org_member — flag values may carry product/strategy hints).
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code = 'features.read'
ON CONFLICT DO NOTHING;

-- features.manage for org_admin / org_owner. Service layer enforces self_service-only for non-operators.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
 WHERE r.code IN ('org_owner', 'org_admin')
   AND p.code = 'features.manage'
ON CONFLICT DO NOTHING;

-- Seed first flag (consumed by upcoming Echo PR).
INSERT INTO feature_definitions (slug, value_type, default_value, description, self_service) VALUES
  ('auth.api_key_headers', 'enum_set',
   '["x-api-key","authorization-bearer"]'::jsonb,
   'Which headers carry API keys for this org. Subset of [x-api-key, authorization-bearer].',
   FALSE)
ON CONFLICT (slug) DO NOTHING;
