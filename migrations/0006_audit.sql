CREATE TABLE audit_events (
    id                       UUID PRIMARY KEY,
    occurred_at              TIMESTAMPTZ NOT NULL,
    recorded_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    category                 TEXT NOT NULL,
    event_type               TEXT NOT NULL,
    actor_user_id            UUID,
    actor_organisation_id    UUID,
    target_type              TEXT,
    target_id                UUID,
    target_organisation_id   UUID,
    request_id               TEXT,
    ip_address               INET,
    user_agent               TEXT,
    outcome                  TEXT NOT NULL,
    reason_code              TEXT,
    payload                  JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX ix_audit_occurred_at   ON audit_events (occurred_at DESC);
CREATE INDEX ix_audit_target_org    ON audit_events (target_organisation_id, occurred_at DESC);
CREATE INDEX ix_audit_actor         ON audit_events (actor_user_id, occurred_at DESC);
CREATE INDEX ix_audit_event_type    ON audit_events (event_type, occurred_at DESC);
