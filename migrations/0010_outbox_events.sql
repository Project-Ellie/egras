CREATE TABLE outbox_events (
    id              UUID PRIMARY KEY,
    aggregate_type  TEXT,
    aggregate_id    UUID,
    event_type      TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    relayed_at      TIMESTAMPTZ,
    relay_attempts  INT NOT NULL DEFAULT 0,
    last_error      TEXT
);

CREATE INDEX ix_outbox_unrelayed
    ON outbox_events (created_at)
    WHERE relayed_at IS NULL;
