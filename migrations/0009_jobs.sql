CREATE TABLE jobs (
    id              UUID PRIMARY KEY,
    kind            TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    state           TEXT NOT NULL,
    attempts        INT  NOT NULL DEFAULT 0,
    max_attempts    INT  NOT NULL,
    run_at          TIMESTAMPTZ NOT NULL,
    locked_until    TIMESTAMPTZ,
    locked_by       TEXT,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT jobs_state_check CHECK (state IN ('pending','running','done','dead'))
);

CREATE INDEX ix_jobs_due
    ON jobs (state, run_at)
    WHERE state IN ('pending','running');

CREATE INDEX ix_jobs_kind_state
    ON jobs (kind, state);
