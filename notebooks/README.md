# egras Scenario Notebooks

Jupyter notebooks that exercise the egras API end-to-end. Each notebook under
`scenarios/` is a runnable scenario and also a pytest test case via `nbmake`.

## Prerequisites

- Python >= 3.11
- PostgreSQL running (see below)
- egras server running with an operator account seeded (see below)

```
pip install -r requirements.txt
```

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `EGRAS_BASE_URL` | `http://localhost:8080` | Where the egras server listens |
| `EGRAS_OPERATOR_EMAIL` | `admin@example.com` | Operator account email |
| `EGRAS_OPERATOR_PASSWORD` | `changeme123` | Operator account password |

Set these to match whatever was passed to `seed-admin` at startup.

## Manual startup sequence

Run these in order before executing any notebook or the pytest suite:

```bash
# 1. Start Postgres
docker-compose up postgres -d

# 2. Seed the operator account
EGRAS_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras \
  cargo run -- seed-admin \
    --email "${EGRAS_OPERATOR_EMAIL:-admin@example.com}" \
    --username admin \
    --password "${EGRAS_OPERATOR_PASSWORD:-changeme123}"

# 3. Start the server
EGRAS_DATABASE_URL=postgres://postgres:secret@localhost:5432/egras \
  EGRAS_CORS_ALLOWED_ORIGINS="*" \
  EGRAS_JWT_SECRET="$(openssl rand -hex 32)" \
  EGRAS_JWT_ISSUER=egras \
  cargo run
```

The server binds to `http://localhost:8080` by default.

## Run a single notebook interactively

```bash
jupyter notebook notebooks/scenarios/01_echo_smoke.ipynb
```

Or with nbmake (executes it as a test):

```bash
pytest --nbmake notebooks/scenarios/01_echo_smoke.ipynb
```

## Run the full suite

```bash
pytest --nbmake notebooks/scenarios
```

The `conftest.py` fixture pings `GET /health` before running any notebook. If
the server is unreachable the whole suite is skipped rather than failed — safe
for CI environments where the server isn't running.
