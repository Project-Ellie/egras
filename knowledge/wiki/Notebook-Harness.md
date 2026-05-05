---
title: Notebook Harness
tags:
  - testing
  - developer-experience
  - api
---

# Notebook Harness

End-to-end scenario tests written as Jupyter notebooks. Each notebook is a narrative — operator login, tenant provisioning, API key minting, request/response assertions — that exercises the full stack against a live egras instance. Lives at `notebooks/` at the repo root.

## Why notebooks (rather than `cargo test`)

Integration tests in `tests/it/` work against `TestPool::fresh()` — a per-test isolated database, with the server router invoked in-process. They are fast, deterministic, and the right tool for verifying behaviour at the unit-and-domain level.

Notebooks are different and complementary:

- **External perspective** — they call the deployed HTTP surface, exactly as a customer's integration would. No private test fixtures, no in-process router shortcuts.
- **Narrative** — readers see the *story* of using the platform: login → provision → restrict → call. Useful onboarding material; useful as documentation that can't drift, because nbmake runs them.
- **Deployable** — a notebook that runs against `localhost:8080` also runs against staging or production with one env-var change. The same scenario is a smoke test, a demo, and a regression check.

They will never replace `cargo test`. The split is: in-process tests for correctness, notebooks for *journey* coverage.

## Layout

```
notebooks/
├── README.md           # prerequisites + how to run
├── requirements.txt    # pinned: requests, jupyter, nbmake, pytest
├── conftest.py         # auto-skips if BASE_URL is unreachable
├── lib/
│   ├── __init__.py
│   └── egras.py        # Client + helper functions (login, create_org, mint_api_key, …)
└── scenarios/
    └── 01_echo_smoke.ipynb
```

Helpers in `lib/egras.py` are thin: the `Client` class is `requests` with auto-injected `Authorization` and `X-API-Key` headers, and the free functions are just typed POST/PUT calls returning the response JSON. No clever abstractions — each scenario should read top-to-bottom as a narrative, with the helpers doing the boring header plumbing only.

## Authoring conventions

- One notebook per scenario; one narrative per notebook.
- Markdown cells set up the story; code cells execute and assert.
- All assertions inline (`assert resp.status_code == 200`) — no helper that hides the contract.
- Negative cases live in the same notebook as the matching positive case (it's part of the same narrative: "this is allowed, this is not").
- Reach for `lib/egras.py` helpers; if you need new HTTP plumbing, add it there rather than inlining `requests.post(...)`.

## Adding a scenario

1. Create `notebooks/scenarios/NN_short_name.ipynb` (NN = sequential number).
2. Use `lib/egras.py` helpers; extend them if a new endpoint shape recurs across scenarios.
3. Run it once interactively (`jupyter notebook`) to confirm green.
4. Run it under nbmake: `pytest --nbmake notebooks/scenarios/NN_short_name.ipynb`.
5. Commit the executed notebook (we don't strip outputs in v1).

## Running the suite

Prerequisites:

```bash
docker-compose up postgres -d
EGRAS_DATABASE_URL=postgres://egras:egras@localhost:15432/egras_notebook \
  cargo run -- seed-admin \
    --email "${EGRAS_OPERATOR_EMAIL:-admin@example.com}" \
    --username admin \
    --password "${EGRAS_OPERATOR_PASSWORD:-changeme123}" \
    --role operator_admin

EGRAS_DATABASE_URL=postgres://egras:egras@localhost:15432/egras_notebook \
  EGRAS_CORS_ALLOWED_ORIGINS=http://localhost:8080 \
  EGRAS_JWT_SECRET="$(openssl rand -hex 32)" \
  EGRAS_JWT_ISSUER=egras \
  cargo run --release
```

Then in another shell:

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r notebooks/requirements.txt
pytest --nbmake notebooks/scenarios
```

`make notebooks-test` is a wrapper for the last command. `make notebooks-up` echoes the boot sequence above (the Makefile deliberately doesn't try to orchestrate cargo + docker — it gets fragile).

## Env vars the harness reads

| Var | Default | Purpose |
|-----|---------|---------|
| `EGRAS_BASE_URL` | `http://localhost:8080` | Where the running egras instance is. |
| `EGRAS_OPERATOR_EMAIL` | `admin@example.com` | Operator identity used to log in. Must match what `seed-admin` was given. |
| `EGRAS_OPERATOR_PASSWORD` | `changeme123` | Operator password. Likewise must match `seed-admin`. |

## Current scenarios

- **`01_echo_smoke.ipynb`** — operator provisions an org, switches into it, creates a service account with `org_admin` (so it inherits `echo:invoke` from migration 0014), mints an API key restricted to `scopes=['echo:invoke']`, calls `POST /api/v1/echo`, asserts the payload round-trips. Two negative checks: a key without `echo:invoke` is rejected with 403; overriding `auth.api_key_headers` to `['x-api-key']` makes `Authorization: Bearer …` requests 401 while `X-API-Key` keeps working. Touches: [[Echo-Service]], [[Service-Accounts]], [[Feature-Flags]].

## Future scope

- Auto-spawn the egras server from `conftest.py` (testcontainers / `subprocess`). Currently a manual prerequisite.
- nbmake job in CI. Add once a second scenario stabilises the harness.
- Stripping notebook outputs in a pre-commit hook. Punted — output diffs are reviewable for now.

## Cross-references

- [[Echo-Service]] — the smoke target the first scenario hits.
- [[Service-Accounts]] — minting API keys, scopes.
- [[Feature-Flags]] — `auth.api_key_headers` allowlist used by the negative case.
- [[Authentication]] — header precedence in the auth middleware.
