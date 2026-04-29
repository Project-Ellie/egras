# egras

**Enterprise-Ready Rust Application Seed** — a production-shaped multi-tenant REST API backend built with Axum, PostgreSQL, and JWT.

Includes: RBAC, audit logging, RFC 7807 error responses, OpenAPI/Swagger UI, and a CLI for bootstrapping.

---

## Quickstart (Docker Compose)

**Prerequisites:** Docker with Compose v2.

```bash
# 1. Clone and start the stack (Postgres + egras)
git clone https://github.com/Project-Ellie/egras.git
cd egras
docker compose up -d

# 2. Seed the first operator admin
docker compose run --rm egras seed-admin \
  --email admin@example.com \
  --username admin \
  --password changeMe123!

# 3. Log in
curl -s -X POST http://localhost:8080/api/v1/security/login \
  -H "Content-Type: application/json" \
  -d '{"username_or_email":"admin@example.com","password":"changeMe123!"}' | jq .
```

The login response contains a `token` (JWT). Pass it as `Authorization: Bearer <token>` on subsequent requests.

Browse the full API at **http://localhost:8080/swagger-ui**.

---

## Local development (without Docker)

**Prerequisites:** Rust stable, PostgreSQL 16.

```bash
# Copy and edit environment
cp .env.example .env          # adjust EGRAS_DATABASE_URL if needed

# Create the database
psql -h localhost -U postgres -c "CREATE DATABASE egras;"

# Run the server (migrations run automatically on startup)
cargo run
```

The server listens on `0.0.0.0:8088` by default (set `EGRAS_BIND_ADDRESS` to override).

---

## Environment variables

All variables are prefixed `EGRAS_`. The only required ones are `EGRAS_DATABASE_URL` and `EGRAS_JWT_SECRET`.

| Variable | Default | Description |
|---|---|---|
| `EGRAS_DATABASE_URL` | *(required)* | PostgreSQL connection string |
| `EGRAS_DATABASE_MAX_CONNECTIONS` | `10` | SQLx pool size |
| `EGRAS_BIND_ADDRESS` | `0.0.0.0:8080` | HTTP listen address |
| `EGRAS_JWT_SECRET` | *(required, ≥32 bytes)* | HMAC-SHA256 signing key — generate with `openssl rand -hex 32` |
| `EGRAS_JWT_TTL_SECS` | `3600` | JWT lifetime in seconds |
| `EGRAS_JWT_ISSUER` | `egras` | JWT `iss` claim |
| `EGRAS_LOG_LEVEL` | `info` | `tracing` filter (e.g. `debug`, `info,sqlx=warn`) |
| `EGRAS_LOG_FORMAT` | `json` | `json` or `pretty` |
| `EGRAS_CORS_ALLOWED_ORIGINS` | *(empty = CORS disabled)* | Comma-separated origins |
| `EGRAS_PASSWORD_RESET_TTL_SECS` | `3600` | Password-reset token lifetime |
| `EGRAS_OPERATOR_ORG_NAME` | `operator` | Name of the super-tenant organisation |
| `EGRAS_AUDIT_CHANNEL_CAPACITY` | `4096` | Audit worker mpsc buffer size |
| `EGRAS_AUDIT_MAX_RETRIES` | `3` | Retries per failed audit write |
| `EGRAS_AUDIT_RETRY_BACKOFF_MS_INITIAL` | `100` | Initial retry backoff (ms, doubles each retry) |

---

## CLI subcommands

```bash
# Run the HTTP server (default)
egras serve

# Seed the first operator admin user (run once after first migration)
egras seed-admin --email <email> --username <username> --password <password> [--role operator_admin]

# Print OpenAPI 3.1 JSON to stdout (used by CI drift check)
egras dump-openapi
```

---

## Running tests

Tests require a separate PostgreSQL database. Each test creates an isolated schema via `TestPool::fresh()`.

```bash
# Start Postgres (if not already running)
docker compose up -d postgres

TEST_DATABASE_URL=postgres://egras:egras@localhost:5432/postgres \
  cargo test --all-features
```

---

## Project structure

```
src/
  security/     # Users, auth, password reset
  tenants/      # Organisations, roles, memberships
  audit/         # Immutable event log
  auth/          # JWT decode, tower middleware, permission extractors
  errors.rs      # AppError → RFC 7807
  app_state.rs   # Dependency injection via trait objects
migrations/     # SQLx migrations (run automatically on startup)
tests/          # Integration and E2E tests
docs/
  openapi.json  # Committed OpenAPI spec (CI checks for drift)
```

---

## Architecture overview

The codebase follows a two-axis layout:

- **Horizontal domains:** `security/`, `tenants/`, `audit/`
- **Vertical layers per domain:** `interface/` → `service/` → `model/` → `persistence/`

Permissions are stored in the DB and loaded per request by the auth middleware. The operator org (`EGRAS_OPERATOR_ORG_NAME`) holds wildcard permissions and can act across all tenants.

See `knowledge/specs/2026-04-18-egras-rust-seed-design.md` for the full design rationale.
