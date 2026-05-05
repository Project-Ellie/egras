# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

**egras** (Enterprise-Ready Rust Application Seed) is a production-shaped foundation for multi-tenant REST API backends: Axum + PostgreSQL + JWT, with RBAC, audit logging, and RFC 7807 error responses. Read `docs/superpowers/specs/2026-04-18-egras-rust-seed-design.md` before any non-trivial work.

## Wiki sync (mandatory)

After any change under `src/`, `migrations/`, or route registration, you MUST update the relevant note(s) in `knowledge/wiki/` in the same commit/PR. Stale wiki docs are a defect. Module-to-note mapping is in `knowledge/wiki/Architecture.md`.

## Commands

```bash
# Format check
cargo fmt --all -- --check

# Lint (warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings

# Run all tests (requires TEST_DATABASE_URL)
TEST_DATABASE_URL=postgres://egras:egras@localhost:15432/postgres \
  cargo test --all-features

# Run a single test
TEST_DATABASE_URL=... cargo test --all-features <test_name>

# Start postgres only
docker-compose up postgres

# Run the server locally
EGRAS_DATABASE_URL=postgres://egras:egras@localhost:15432/egras \
  cargo run
```

## Architecture

The codebase is divided along two axes:

**Horizontal domains:** `security/`, `tenants/`, `audit/`  
**Vertical layers per domain:**
1. `interface/` — Axum handlers + request/response DTOs
2. `service/` — One file per use case; contains business logic
3. `model/` — Domain types and value objects
4. `persistence/` — Repository traits + `*_pg.rs` PostgreSQL implementations

**Cross-cutting modules** in `src/`:
- `auth/` — JWT decode, tower middleware, permission loading
- `app_state.rs` — Trait objects injected via `AppState`; dependency injection pattern
- `errors.rs` — `AppError` enum + RFC 7807 `ProblemResponse` formatting
- `audit/worker.rs` — Background task drains mpsc channel → persists audit events
- `testing.rs` — Feature-gated: `TestPool::fresh()` (isolated per-test DB), `BlockingAuditRecorder`, JWT helpers

## Key conventions

- **IDs:** UUID v7 (time-ordered, application-generated, never database-generated)
- **Timestamps:** `chrono::DateTime<Utc>`, serialized as RFC 3339
- **Config:** env vars prefixed `EGRAS_`; parsed via figment
- **Permissions:** stored in DB; loaded per request by auth middleware; enforced via `RequirePermission` extractor
- **Org scoping:** cross-tenant access returns 404 (hides tenant existence); operator org has wildcard permissions
- **Audit:** all state-changing handlers emit an `AuditEvent` via `AppState::audit_recorder()`; use `BlockingAuditRecorder` in tests to assert on emitted events
- **Error slugs:** defined as constants on `AppError`; keep slugs stable (API contract)
- **OpenAPI:** `docs/openapi.json` is committed and must be regenerated after handler changes (`cargo run -- dump-openapi > docs/openapi.json`)

## Testing strategy

Tests live in `/tests/` organized by domain and layer (`service/`, `persistence/`, `interface/`). Every new use case needs tests at all three levels. Interface tests spin up a bound local port and use `reqwest` against it. `TestPool::fresh()` creates an isolated database per test—do not share pools across tests.

## Implementation status

- **Plan 1 + 2a (merged):** Tenants domain, auth middleware, audit infrastructure
- **Plan 2b (merged):** Security domain (register, login, logout, change-password)
- **Plan 3 (merged):** `seed-admin` and `dump-openapi` CLI subcommands
- **Plan 4 (merged):** OpenAPI drift check in CI, README quickstart

<!-- icm:start -->
## Persistent memory (ICM) — MANDATORY

This project uses [ICM](https://github.com/rtk-ai/icm) for persistent memory across sessions.
You MUST use it actively. Not optional.

### Recall (before starting work)
```bash
icm recall "query"                        # search memories
icm recall "query" -t "topic-name"        # filter by topic
icm recall-context "query" --limit 5      # formatted for prompt injection
```

### Store — MANDATORY triggers
You MUST call `icm store` when ANY of the following happens:
1. **Error resolved** → `icm store -t errors-resolved -c "description" -i high -k "keyword1,keyword2"`
2. **Architecture/design decision** → `icm store -t decisions-{project} -c "description" -i high`
3. **User preference discovered** → `icm store -t preferences -c "description" -i critical`
4. **Significant task completed** → `icm store -t context-{project} -c "summary of work done" -i high`
5. **Conversation exceeds ~20 tool calls without a store** → store a progress summary

Do this BEFORE responding to the user. Not after. Not later. Immediately.

Do NOT store: trivial details, info already in CLAUDE.md, ephemeral state (build logs, git status).

### Other commands
```bash
icm update <id> -c "updated content"     # edit memory in-place
icm health                                # topic hygiene audit
icm topics                                # list all topics
```
<!-- icm:end -->
