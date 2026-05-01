---
title: Configuration
tags:
  - config
  - environment
  - deployment
---

# Configuration

All configuration is read from environment variables prefixed `EGRAS_`, loaded via [figment](https://docs.rs/figment). A `.env` file in the working directory is also supported (via `dotenvy`).

Configuration logic lives in [`src/config.rs`](../../src/config.rs).

## Required Variables

These must be set or the application will fail to start:

| Variable | Description |
|----------|-------------|
| `EGRAS_DATABASE_URL` | PostgreSQL connection string, e.g. `postgres://user:pass@host:5432/dbname` |
| `EGRAS_JWT_SECRET` | HS256 signing secret — **must be ≥ 32 bytes**. Validated on startup. |
| `EGRAS_CORS_ALLOWED_ORIGINS` | Comma-separated allowed origins, e.g. `https://app.example.com,https://admin.example.com`. Must be set — an empty value will fail startup. |

## Optional Variables (with defaults)

| Variable | Default | Description |
|----------|---------|-------------|
| `EGRAS_BIND_ADDRESS` | `0.0.0.0:8080` | HTTP listen address |
| `EGRAS_DATABASE_MAX_CONNECTIONS` | `10` | sqlx connection pool size |
| `EGRAS_JWT_TTL_SECS` | `3600` | JWT lifetime in seconds (1 hour) |
| `EGRAS_JWT_ISSUER` | `egras` | `iss` claim in issued tokens |
| `EGRAS_LOG_LEVEL` | `info` | tracing log filter (e.g. `debug`, `warn`) |
| `EGRAS_LOG_FORMAT` | `json` | `json` (structured) or `pretty` (human-readable) |
| `EGRAS_PASSWORD_RESET_TTL_SECS` | `3600` | Password reset token lifetime |
| `EGRAS_OPERATOR_ORG_NAME` | `operator` | Name of the operator organisation in the DB |
| `EGRAS_AUDIT_CHANNEL_CAPACITY` | `4096` | Audit mpsc channel buffer size |
| `EGRAS_AUDIT_MAX_RETRIES` | `3` | Retry count for failed audit DB writes |
| `EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL` | `100` | Initial backoff ms for audit retry; doubles each attempt |

## Startup Validation

`AppConfig::from_env()` calls `validate()` after loading. It fails immediately (before starting the HTTP server) if:

- `EGRAS_JWT_SECRET` is shorter than 32 bytes
- `EGRAS_LOG_FORMAT` is not `json` or `pretty`
- `EGRAS_CORS_ALLOWED_ORIGINS` is empty or whitespace

This is a deliberate fail-fast approach — a misconfigured service should crash loudly at startup rather than behave incorrectly at runtime.

## AppConfig Struct

```rust
pub struct AppConfig {
    pub database_url:                    String,
    pub database_max_connections:        u32,
    pub bind_address:                    String,
    pub jwt_secret:                      String,
    pub jwt_ttl_secs:                    i64,
    pub jwt_issuer:                      String,
    pub log_level:                       String,
    pub log_format:                      String,
    pub cors_allowed_origins:            String,
    pub password_reset_ttl_secs:         i64,
    pub operator_org_name:               String,
    pub audit_channel_capacity:          usize,
    pub audit_max_retries:               u32,
    pub audit_retry_backoff_ms_initial:  u64,
}
```

## CORS

`EGRAS_CORS_ALLOWED_ORIGINS` is a comma-separated list of origin strings. Each is parsed as an `http::HeaderValue`. Invalid entries are silently dropped; if the resulting list is empty after parsing, `build_cors()` returns an error.

Allowed methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`.  
Allowed headers: `Content-Type`, `Authorization`.

## Bootstrap: seed-admin

After running migrations for the first time, you need to create the first operator admin user:

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@localhost:5432/egras \
EGRAS_JWT_SECRET=<your-32+-byte-secret> \
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000 \
./egras seed-admin \
  --email admin@example.com \
  --username admin \
  --password "YourSecurePassword!" \
  --role operator_admin
```

The CLI subcommand:
1. Looks up the operator org by `EGRAS_OPERATOR_ORG_NAME`
2. Validates the email doesn't already exist
3. Hashes the password with argon2id
4. Inserts the user + org membership in a transaction
5. Writes a `user.registered` audit event synchronously

Once the seed admin exists, all subsequent user registration is done via the REST API using their JWT.

## Example .env for local development

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@127.0.0.1:5432/egras
EGRAS_JWT_SECRET=dev-only-32-bytes-of-placeholder-xx
EGRAS_BIND_ADDRESS=127.0.0.1:8080
EGRAS_LOG_FORMAT=pretty
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000
EGRAS_LOG_LEVEL=debug
```

## Related notes

- [[CI-and-Deployment]] — how config is supplied in Docker Compose and GitHub Actions
- [[Architecture]] — how `AppConfig` flows into `build_app()`
- [[Audit-System]] — the audit channel config vars
