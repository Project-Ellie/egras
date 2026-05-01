---
title: CI and Deployment
tags:
  - ci
  - deployment
  - docker
  - github-actions
---

# CI and Deployment

## GitHub Actions Pipeline

The CI workflow lives at [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml). It runs on every push to `main` and every pull request targeting `main`.

### Steps

```
1. Checkout
2. Install Rust toolchain (stable + rustfmt + clippy)
3. Cache cargo registry & build artifacts (Swatinem/rust-cache)
4. cargo fmt --all -- --check
5. cargo clippy --all-targets --all-features -- -D warnings
6. cargo test --all-features
7. cargo build --release
8. openapi drift check
```

### Step 4 — Format check

Zero tolerance for formatting deviations. If `rustfmt` would change any file, CI fails. Run `cargo fmt --all` before pushing.

> [!tip] Common trap
> `rustfmt` reorders `use` declarations alphabetically. If you add a `use` statement by hand or via sed, it may land out of order. Always run `cargo fmt` before committing.

### Step 5 — Clippy

All clippy warnings are treated as errors (`-D warnings`). Common sources of failures:
- Unused variables, imports, or dead code
- Needless `return` / `clone`
- `items_after_test_module` — `#[cfg(test)]` module must be at the bottom of the file

### Step 6 — Tests

Runs the full test suite against a PostgreSQL 16 service container.

```yaml
services:
  postgres:
    image: postgres:16-alpine
    env:
      POSTGRES_USER: egras
      POSTGRES_PASSWORD: egras
      POSTGRES_DB: postgres   # ← note: "postgres", not "egras"
    ports:
      - 15432:5432
    options: >-
      --health-cmd "pg_isready -U egras"
      --health-interval 5s
      --health-retries 20
```

`TEST_DATABASE_URL` is set to `postgres://egras:egras@127.0.0.1:15432/postgres`. Each test creates its own isolated database.

### Step 7 — Release build

Compiles with optimisations. Catches any code that compiles in debug but not release (rare, but possible with conditional compilation).

The `Cargo.toml` release profile uses:
```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

### Step 8 — OpenAPI drift check

```bash
EGRAS_DATABASE_URL=postgres://egras:egras@127.0.0.1:15432/postgres \
EGRAS_JWT_SECRET=ci-only-32-bytes-placeholder-xxxxx \
EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:3000 \
  ./target/release/egras dump-openapi > target/openapi-ci.json
diff docs/openapi.json target/openapi-ci.json
```

If the generated spec differs from the committed `docs/openapi.json`, CI fails. This prevents handler changes from going out without updating the spec.

After any handler change, regenerate locally and commit:

```bash
EGRAS_DATABASE_URL=... EGRAS_JWT_SECRET=... EGRAS_CORS_ALLOWED_ORIGINS=... \
  cargo run -- dump-openapi > docs/openapi.json
git add docs/openapi.json
```

---

## Docker Compose

[`docker-compose.yml`](../../docker-compose.yml) defines two services for local development:

### postgres service

```yaml
postgres:
  image: postgres:16-alpine
  environment:
    POSTGRES_USER: egras
    POSTGRES_PASSWORD: egras
    POSTGRES_DB: egras
  ports:
    - "5432:5432"
  healthcheck:
    test: ["CMD", "pg_isready", "-U", "egras"]
    interval: 5s
    timeout: 3s
    retries: 20
```

### egras service

```yaml
egras:
  build: .
  depends_on:
    postgres:
      condition: service_healthy
  environment:
    EGRAS_DATABASE_URL: postgres://egras:egras@postgres:5432/egras
    EGRAS_JWT_SECRET: DEV-ONLY-32-bytes-of-placeholder-entropy
    EGRAS_BIND_ADDRESS: 0.0.0.0:8080
    EGRAS_LOG_FORMAT: json
    EGRAS_CORS_ALLOWED_ORIGINS: http://localhost:3000
  ports:
    - "8080:8080"
```

Startup sequence:
1. Postgres starts and passes health check
2. egras container builds and starts
3. Migrations run automatically
4. HTTP server binds at `0.0.0.0:8080`

### Common commands

```bash
# Start everything
docker-compose up

# Start only Postgres (develop against it with cargo run)
docker-compose up postgres

# Rebuild the egras image after code changes
docker-compose up --build egras

# Tear down including volumes (destructive!)
docker-compose down -v
```

---

## Health Endpoints

The server exposes two health endpoints, both unauthenticated:

| Endpoint | Returns | Purpose |
|----------|---------|---------|
| `GET /health` | `{"status":"ok"}` (200) | Liveness — is the process running? |
| `GET /ready` | `{"status":"ready"}` or 503 | Readiness — can it serve traffic? (checks DB) |

Use `/ready` in Kubernetes readiness probes; use `/health` for liveness probes.

---

## Logging

Structured logging via `tracing` + `tracing-subscriber`.

- `EGRAS_LOG_LEVEL` controls the filter (default `info`)
- `EGRAS_LOG_FORMAT=json` — machine-readable, for log aggregators (Datadog, CloudWatch, etc.)
- `EGRAS_LOG_FORMAT=pretty` — human-readable, for local development

Audit events are always logged at `INFO` to the `egras::audit` target in addition to being written to the database. This dual-write means log aggregators can be queried for audit data even if the DB is unavailable.

---

## Secrets Management

In production, replace `.env` / hardcoded values with your secrets manager of choice:

| Secret | Env var | Notes |
|--------|---------|-------|
| Database password | `EGRAS_DATABASE_URL` | Full connection string; use IAM auth or vault injection |
| JWT signing key | `EGRAS_JWT_SECRET` | ≥32 bytes; rotate requires re-issuing all tokens |

> [!danger] Never commit secrets
> `EGRAS_JWT_SECRET` in source code or docker-compose.yml is acceptable for local development only. Use environment injection (Kubernetes secrets, AWS SSM, Vault) in staging and production.

---

## Related notes

- [[Configuration]] — all `EGRAS_*` environment variables
- [[Developer-Guide]] — pre-push checklist and local setup
- [[Architecture]] — how `build_app()` is the entry point for the server
