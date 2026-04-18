CREATE TABLE organisations (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    business        TEXT NOT NULL,
    is_operator     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX ux_organisations_operator
    ON organisations (is_operator) WHERE is_operator = TRUE;
