---
title: Testing Strategy
tags:
  - testing
  - tdd
  - integration
---

# Testing Strategy

Tests in egras are integration-heavy by design. The layered [[Architecture]] enables testing each layer in isolation with real dependencies at the appropriate level.

All test infrastructure is in [`src/testing.rs`](../../src/testing.rs) (feature-gated behind `cfg(any(test, feature = "testing"))`) and [`tests/it/common/`](../../tests/it/common/).

> [!note] Single integration-test binary
> Every `tests/it/<name>_test.rs` is a module of one `it` binary declared in [`tests/it/main.rs`](../../tests/it/main.rs) and registered via the `[[test]]` stanza in `Cargo.toml` (with `autotests = false` so cargo doesn't auto-discover loose files). This means **42+ integration files link once, not 42+ times** — full builds drop from minutes to seconds. Keep the `_test.rs` suffix for traceability; just add the new file under `tests/it/` and register it with `mod <name>;` in `tests/it/main.rs`. From a sibling test, refer to shared helpers as `crate::common::...`.

## Three Test Layers

### Layer 1 — Persistence Tests

Files: `tests/it/*_persistence_test.rs`

Test the repository implementations against a real PostgreSQL database. No mocking — if a query is wrong, the test fails.

```rust
// tests/security_persistence_test.rs
#[tokio::test]
async fn create_user_and_find_by_email() {
    let pool = TestPool::fresh().await.pool;
    let repo = UserRepositoryPg::new(pool);

    let user = repo.create("alice", "alice@example.com", "hash").await.unwrap();
    let found = repo.find_by_username_or_email("alice@example.com").await.unwrap();

    assert_eq!(found.unwrap().id, user.id);
}
```

**What they cover:** SQL correctness, constraint enforcement, transaction atomicity, index behaviour.

### Layer 2 — Service Tests

Files: `tests/it/*_service_*_test.rs`

Test the use-case functions. These use a real DB (via `TestPool`) but through `MockAppStateBuilder`, which wires the state with either real Postgres repos or mocks depending on what the test needs.

```rust
// tests/security_service_login_test.rs
#[tokio::test]
async fn login_with_wrong_password_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;

    let state = MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_security_repos()
        .build();

    let err = login(&state, LoginInput {
        username_or_email: "alice".into(),
        password: "wrong".into(),
    })
    .await
    .unwrap_err();

    assert!(matches!(err, LoginError::InvalidCredentials));
}
```

**What they cover:** Business logic, error mapping, audit event emission.

### Layer 3 — HTTP / Interface Tests

Files: `tests/it/*_http_*_test.rs`

Full end-to-end over HTTP. A real server is bound to a random port; tests use `reqwest` to make requests against it. Assertions cover status codes, response body shapes, and headers.

```rust
// tests/security_http_login_test.rs
#[tokio::test]
async fn login_returns_token_and_memberships() {
    let pool = TestPool::fresh().await.pool;
    let app = TestApp::spawn(pool.clone(), default_config()).await;

    // Seed
    seed_user_with_org(&pool, "alice", "alice@example.com", "pass123").await;

    let resp = app.client
        .post(format!("{}/api/v1/security/login", app.base_url))
        .json(&json!({ "username_or_email": "alice", "password": "pass123" }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert!(!body["memberships"].as_array().unwrap().is_empty());

    app.stop().await;
}
```

**What they cover:** Full request path, permission enforcement by middleware, response format, CORS headers, error responses.

### Layer 4 — End-to-End Tests

Files: `tests/it/e2e_*_test.rs`

Multi-step user journeys — e.g., bootstrap via CLI, then login via HTTP, then perform operations.

## Test Infrastructure

### TestPool

```rust
let pool = TestPool::fresh().await.pool;
```

`TestPool::fresh()` creates an isolated PostgreSQL schema for each test:
1. Connects to the admin URL (`TEST_DATABASE_URL`)
2. Creates a uniquely-named database (UUID suffix)
3. Runs all migrations against it
4. Returns a `PgPool` connected to that database

Each test gets its own clean schema. Tests can run in parallel without interfering. The schema is not cleaned up automatically — it is left for debugging.

### MockAppStateBuilder

```rust
let state = MockAppStateBuilder::new(pool.clone())
    .with_blocking_audit()        // synchronous audit writes (for assertions)
    .with_pg_tenants_repos()      // real Postgres org + role repos
    .with_pg_security_repos()     // real Postgres user + token repos
    .with_jwt_config(config)      // optional custom JWT settings
    .build();
```

Builds an `AppState` suitable for service tests. Every `.with_*()` method wires in real or mock implementations.

### BlockingAuditRecorder

In production, audit events are written asynchronously via an mpsc channel. In tests, that would make assertions on audit events non-deterministic. `BlockingAuditRecorder` writes directly to the DB synchronously inside `record()`, so after a service call returns, the audit row is already in the database:

```rust
let state = MockAppStateBuilder::new(pool.clone())
    .with_blocking_audit()
    .build();

login(&state, input).await.unwrap();

// Immediately queryable:
let count: i64 = sqlx::query_scalar(
    "SELECT COUNT(*) FROM audit_events WHERE event_type = 'login.success'"
).fetch_one(&pool).await.unwrap();
assert_eq!(count, 1);
```

### TestApp

```rust
let app = TestApp::spawn(pool.clone(), config).await;
// app.base_url → "http://127.0.0.1:<random_port>"
// app.client   → reqwest::Client with base URL preset
app.stop().await;
```

`TestApp` spawns a real Axum server bound to port 0 (OS assigns a free port). All middleware (auth, CORS, tracing) runs as normal. This is the highest-fidelity test environment.

### Seed Helpers

`tests/it/common/seed.rs` provides helpers to insert test data:

```rust
let user_id = seed_user(&pool, "alice").await;
let org_id  = seed_org(&pool, "acme", "retail").await;
seed_membership(&pool, user_id, org_id, "org_member").await;
let token   = mint_jwt_for(&pool, user_id, org_id).await;
```

### JWT Helpers

Tests that need an authenticated request use the JWT minting helpers in `tests/it/common/auth.rs`:

```rust
let token = mint_jwt(user_id, org_id, &jwt_config);
let resp = app.client
    .get(url)
    .bearer_auth(token)
    .send().await.unwrap();
```

## Running Tests

```bash
# Full suite (requires Postgres) — preferred runner
TEST_DATABASE_URL=postgres://egras:egras@127.0.0.1:15432/postgres \
  cargo nextest run --all-features

# All tests in one module (file)
TEST_DATABASE_URL=... cargo nextest run --all-features security_service_login_test

# Single test function
TEST_DATABASE_URL=... cargo nextest run --all-features login_with_wrong_password_returns_error

# Stock cargo equivalent (slightly less parallel, no per-test timing summary)
TEST_DATABASE_URL=... cargo test --all-features

# With output
TEST_DATABASE_URL=... cargo test --all-features -- --nocapture
```

`cargo nextest` is recommended — install once with `cargo install cargo-nextest --locked`. It has the same semantics as `cargo test` but parallelises better and prints a per-test timing summary; on this codebase it brings the full suite to ~8 s.

> [!tip] `serial_test`
> Some tests set environment variables (e.g., config tests). These use `#[serial_test::serial]` to prevent parallel execution from causing interference. If you add a test that modifies global state, add `#[serial]`.

## Test File Organisation

All integration tests live under `tests/it/` and link into one `it` binary declared in [`tests/it/main.rs`](../../tests/it/main.rs). Cargo discovers nothing in `tests/` automatically — `autotests = false` plus the explicit `[[test]] name = "it" path = "tests/it/main.rs"` stanza in `Cargo.toml` is the source of truth.

```
tests/
└─ it/
    ├─ main.rs                      Lists every `mod foo_test;`
    ├─ common/
    │   ├─ mod.rs                   Shared helpers (re-exports the three submodules)
    │   ├─ seed.rs                  Data seeding helpers
    │   └─ auth.rs                  JWT minting helpers
    │
    ├─ security_persistence_test.rs
    ├─ security_service_*_test.rs   (login, register, list_users, ...)
    ├─ security_http_*_test.rs
    │
    ├─ tenants_persistence_test.rs
    ├─ tenants_service_*_test.rs
    ├─ tenants_http_*_test.rs
    │
    ├─ audit_*_test.rs
    ├─ jobs_*_test.rs
    ├─ outbox_*_test.rs
    ├─ auth_*_test.rs
    │
    ├─ bootstrap_seed_admin_test.rs
    ├─ config_test.rs
    ├─ errors_test.rs
    ├─ health_test.rs
    └─ e2e_*_test.rs
```

### Adding a new integration test

1. Create `tests/it/<name>_test.rs` with `#[tokio::test]` functions.
2. Add `mod <name>_test;` in `tests/it/main.rs` (alphabetical for tidiness).
3. Inside the new file, refer to shared helpers as `use crate::common::seed::seed_user;` etc. — `common` is declared at the crate root in `main.rs`, so siblings reach it via `crate::`.

## Related notes

- [[Architecture]] — the layered structure that enables these test patterns
- [[Developer-Guide]] — how to write tests for a new use case
