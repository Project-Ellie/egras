# Echo Service + Jupyter Scenario Harness — Plan

**Goal:** trivial `/v1/echo` endpoint guarded by per-key API auth, plus reusable Python/Jupyter harness that runs end-to-end scenarios against a live egras server. First scenario validates the API-key flow shipped in #13.

**Architecture:**
- Rust side: new `echo/` domain, no persistence, no migrations. Reuses existing API-key middleware + `RequirePermission` extractor. Permission `echo:invoke`.
- Python side: `notebooks/` tree with shared client lib (`requests`-based), per-scenario notebook, `pytest --nbmake` runner. Server assumed already running on `localhost:8080` (a Make target spins up postgres + egras).
- Wiki: new `Echo-Service.md`, promoted `Notebook-Harness.md`, INDEX strikethroughs.

**Tech:** Rust (axum, sqlx already in tree). Python ≥3.11, `requests`, `jupyter`, `nbmake`, `pytest`.

---

## File map

**Rust (new):**
- `src/echo/mod.rs` — module wiring
- `src/echo/interface.rs` — `get_echo`, `post_echo` handlers + DTOs + router
- `src/echo/service.rs` — pure logic (build response payload). Trivial; keep separate so test-at-service-layer is possible later
- `tests/it/echo/mod.rs` — interface-level tests
- `migrations/` — none
- `knowledge/wiki/Echo-Service.md` — module note
- `knowledge/wiki/future-enhancements/Notebook-Harness.md` — promoted from draft

**Rust (modify):**
- `src/lib.rs` — `pub mod echo;`
- `src/main.rs` or wherever the router is composed — mount echo router under `/v1/echo`
- `src/auth/permissions.rs` — register `echo:invoke` constant if permissions are enumerated; otherwise just document the slug
- `tests/it/main.rs` — `mod echo;`
- `docs/openapi.json` — regenerated
- `knowledge/wiki/Architecture.md` — add echo to module map
- `knowledge/wiki/future-enhancements/INDEX.md` — strike `integration-test-notebooks` once first notebook green; add `Echo-Service` line under Identity/Access (or new "Examples" section)
- delete: `knowledge/wiki/future-enhancements/integration-test-notebooks.md` (replaced by `Notebook-Harness.md`)

**Python (new):**
- `notebooks/README.md` — prereqs, how to run
- `notebooks/requirements.txt` — pinned: `requests`, `jupyter`, `nbmake`, `pytest`
- `notebooks/lib/egras.py` — `Client`, `bootstrap_org_with_service_account()`, `mint_api_key()`, login helpers
- `notebooks/conftest.py` — server-up fixture (skip-if-down, no auto-start in v1)
- `notebooks/scenarios/01_echo_smoke.ipynb` — first narrative
- `Makefile` (or extend if exists) — `make notebooks-up`, `make notebooks-test`

---

## Tasks

### Task 1 — Echo handler (Rust, TDD)

Files: `src/echo/{mod,interface,service}.rs`, `tests/it/echo/mod.rs`, `src/lib.rs`, router composition site.

- [ ] **1.1** Write failing interface test: `POST /v1/echo` with valid API key + `echo:invoke` permission → 200, body `{"method":"POST","payload":<sent>,"org_id":...,"key_id":...}`. Same key without permission → 403. No key → 401.
- [ ] **1.2** Add `src/echo/service.rs` — `fn build_echo(method: &str, body: serde_json::Value, ctx: &AuthContext) -> EchoResponse`.
- [ ] **1.3** Add `src/echo/interface.rs` — handlers `get_echo`/`post_echo`, `pub fn router() -> Router<AppState>`. Permission via `RequirePermission("echo:invoke")`.
- [ ] **1.4** Wire `pub mod echo;` in `src/lib.rs`; mount `.nest("/v1/echo", echo::interface::router())` at the router builder.
- [ ] **1.5** `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run --all-features` — green.
- [ ] **1.6** `cargo run -- dump-openapi > docs/openapi.json`.
- [ ] **1.7** Commit: `feat(echo): add /v1/echo endpoint guarded by echo:invoke permission`.

### Task 2 — Wiki note for Echo

- [ ] **2.1** Create `knowledge/wiki/Echo-Service.md` — purpose (notebook target), endpoint shapes, permission slug, auth model, link to `Service-Accounts.md`.
- [ ] **2.2** Edit `knowledge/wiki/Architecture.md` — add row: `echo/` → `Echo-Service.md`.
- [ ] **2.3** Commit: `docs(wiki): document Echo service`.

### Task 3 — Python harness scaffolding

- [ ] **3.1** `notebooks/requirements.txt`:
  ```
  requests==2.32.3
  jupyter==1.1.1
  nbmake==1.5.4
  pytest==8.3.3
  ```
- [ ] **3.2** `notebooks/lib/egras.py`:
  - `class Client(base_url, *, jwt=None, api_key=None)` with `.get/.post/.put/.delete` + auto-header injection (`Authorization: Bearer …` for JWT, `X-API-Key: …` for API keys — match header names egras actually expects; verify in `src/auth/middleware.rs`).
  - `def login_operator(base_url, email, password) -> str` (returns JWT).
  - `def create_org(client, name) -> dict`.
  - `def create_service_account(client, org_id, name) -> dict`.
  - `def mint_api_key(client, org_id, sa_id, *, scopes) -> tuple[str, dict]` (plaintext key, metadata).
  - `def grant_permission(client, principal_id, slug)` — only if needed; otherwise pass scopes at mint time.
- [ ] **3.3** `notebooks/conftest.py` — fixture that pings `GET /healthz` (or whatever exists), `pytest.skip` if unreachable. No auto-spawn in v1.
- [ ] **3.4** `notebooks/README.md` — prereqs (postgres up, egras running with seeded operator, `pip install -r requirements.txt`), how to run a notebook, how to run the suite (`pytest --nbmake notebooks/scenarios`).
- [ ] **3.5** `Makefile` targets:
  - `notebooks-up`: docker-compose up postgres -d; cargo run -- seed-admin; cargo run & (or instructions only — pick whichever is reliable).
  - `notebooks-test`: `pytest --nbmake notebooks/scenarios`.
- [ ] **3.6** Commit: `feat(notebooks): scaffold Python/Jupyter scenario harness`.

### Task 4 — First scenario: echo smoke

- [ ] **4.1** `notebooks/scenarios/01_echo_smoke.ipynb` cells, in order:
  1. Markdown: narrative — "Operator boots, provisions an org, creates a service account, mints an API key with `echo:invoke`, calls echo, asserts payload round-trips."
  2. Code: imports + `BASE = "http://localhost:8080"`.
  3. Code: `op = Client(BASE, jwt=login_operator(BASE, OPERATOR_EMAIL, OPERATOR_PW))`.
  4. Code: `org = create_org(op, "AcmeCorp")`.
  5. Code: `sa = create_service_account(op, org["id"], "echo-bot")`.
  6. Code: `plaintext, meta = mint_api_key(op, org["id"], sa["id"], scopes=["echo:invoke"])`.
  7. Code: `caller = Client(BASE, api_key=plaintext); resp = caller.post("/v1/echo", json={"hello": "world"})`.
  8. Code: assertions — `resp.status_code == 200`; `resp.json()["payload"] == {"hello": "world"}`; `resp.json()["org_id"] == org["id"]`.
  9. Markdown: negative case header.
  10. Code: mint a second key WITHOUT `echo:invoke` → assert 403.
- [ ] **4.2** Run notebook end-to-end manually; confirm green.
- [ ] **4.3** Run `pytest --nbmake notebooks/scenarios/01_echo_smoke.ipynb` — green.
- [ ] **4.4** Commit: `test(notebooks): first scenario — echo smoke`.

### Task 5 — Promote draft + INDEX update

- [ ] **5.1** Move/rewrite `knowledge/wiki/future-enhancements/integration-test-notebooks.md` → `knowledge/wiki/future-enhancements/Notebook-Harness.md`. Document: directory layout, scenario authoring conventions (one narrative per notebook, uses `lib/egras.py`, asserts inline), how to add a scenario, link to first one.
- [ ] **5.2** Edit `knowledge/wiki/future-enhancements/INDEX.md`: strikethrough `Notebook-Harness` (it's now shipped infra), and either strikethrough or note Echo-Service as an example endpoint, not a real feature.
- [ ] **5.3** Commit: `docs(wiki): promote notebook harness, mark shipped`.

### Task 6 — Pre-push gate + push + PR

- [ ] **6.1** `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run --all-features` green.
- [ ] **6.2** Branch + push + open PR titled `feat: echo endpoint + Jupyter scenario harness`.
- [ ] **6.3** Poll CI; iterate if red.

---

## Out of scope (deferred, do not touch this PR)
- Auto-spawning the server from `conftest.py` (testcontainers / `subprocess`). Manual prereq for v1.
- nbmake job in GitHub Actions. Add only after second scenario lands and the harness is stable.
- More scenarios beyond echo (login flows, channels, jobs, outbox). Each gets its own follow-up PR.
- Notebook output stripping / pre-commit hook. Punt.

---

## Unresolved questions
1. **API-key header name.** `X-API-Key`, `Authorization: Bearer …`, or both? Need to read `src/auth/middleware.rs` before writing `lib/egras.py`. Cheap to check; flagging so plan-reader doesn't guess.
2. **Permission granting model.** Is `echo:invoke` granted via API-key `scopes` at mint time, or via a separate `grant_permission` on the service account, or both? Determines `lib/egras.py` shape. (My current draft assumes scopes-at-mint.)
3. **Operator credentials in notebooks.** Hardcode dev defaults from `seed-admin` (visible in repo, fine for local), or read from env? Vote: env with documented defaults in README.
4. **Should the echo response also include the request headers?** Useful for "see what the server sees" debugging, but leaks. Default: no — only method, payload, org_id, key_id, timestamp.
5. **Echo permission slug naming.** `echo:invoke` vs `echo:call` vs reuse a generic `service:invoke`. Trivial but commits to a convention. I picked `echo:invoke`.
6. **First-notebook scope creep.** Tempting to also exercise login + JWT in the same notebook. Recommendation: keep #1 minimal, add `02_auth_flows.ipynb` next.
