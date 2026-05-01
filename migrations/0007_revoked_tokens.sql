CREATE TABLE revoked_tokens (
    jti         UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ NOT NULL
);

-- Allows periodic pruning of expired entries and quick expiry checks.
CREATE INDEX ix_revoked_tokens_expires_at ON revoked_tokens (expires_at);
