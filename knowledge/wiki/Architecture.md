---
title: Architecture
tags:
  - architecture
  - design
---

# Architecture

egras is organised along two orthogonal axes: **horizontal domains** and **vertical layers**. Every piece of code belongs to exactly one cell in this grid.

## The 2D Grid

```
             interface/   service/    model/    persistence/
             (HTTP)        (logic)    (types)     (DB)
             ─────────────────────────────────────────────
security/  │  handlers   login.rs    User       UserRepositoryPg
tenants/   │  handlers   create_org  Org        OrgRepositoryPg
audit/     │  handlers   list_evts   AuditEvent AuditRepositoryPg
```

### Horizontal: Domains

| Domain | Responsibility |
|--------|---------------|
| `features/` | Org-level feature flags — definitions, overrides, evaluation |
| `security/` | Users, authentication, password management |
| `tenants/` | Organisations, memberships, role assignment |
| `audit/` | Immutable event log — writing and reading |

### Vertical: Layers (per domain)

| Layer | Path pattern | Contains |
|-------|-------------|---------|
| **interface** | `src/<domain>/interface.rs` | Axum handlers, request/response DTOs, route registration |
| **service** | `src/<domain>/service/<use_case>.rs` | One file per use case; all business logic lives here |
| **model** | `src/<domain>/model.rs` | Domain types, value objects, cursors |
| **persistence** | `src/<domain>/persistence/` | Repository traits + `*_pg.rs` sqlx implementations |

> [!note] One file = one use case
> Each service file (e.g., [`src/security/service/login.rs`](../../src/security/service/login.rs)) exports exactly: `Input`, `Output`, `Error`, and a single async function. This makes every use case self-contained and independently testable.

### Cross-cutting modules

These live directly in `src/` and are shared across all domains:

| Module                                         | Purpose                                                 |
| ---------------------------------------------- | ------------------------------------------------------- |
| [`src/auth/`](../../src/auth/)                 | JWT encode/decode, tower middleware, permission loading |
| [`src/app_state.rs`](../../src/app_state.rs)   | Dependency injection container (`AppState`)             |
| [`src/config.rs`](../../src/config.rs)         | Env-var loading and validation                          |
| [`src/errors.rs`](../../src/errors.rs)         | `AppError` enum → RFC 7807 JSON responses               |
| [`src/pagination.rs`](../../src/pagination.rs) | Cursor codec shared across paginated endpoints          |
| [`src/db.rs`](../../src/db.rs)                 | Pool construction, migration runner                     |
| [`src/lib.rs`](../../src/lib.rs)               | `build_app()` — assembles the full Axum router          |
| [`src/jobs/`](../../src/jobs/)                 | Durable background-job queue + runner — see [[Jobs]]    |
| [`src/outbox/`](../../src/outbox/)             | Transaction-coupled event outbox + relayer — see [[Outbox]] |
| `src/security/service/{create,list,delete}_service_account.rs` and `*_api_key.rs` | Service accounts (non-human principals) + per-key API keys — see [[Service-Accounts]] |
| [`src/openapi.rs`](../../src/openapi.rs)       | OpenAPI 3.1 schema via utoipa                           |

## Dependency Injection via `AppState`

Every handler receives `State<AppState>` from Axum. `AppState` holds `Arc<dyn Trait>` for every service and repository:

```rust
// src/app_state.rs
pub struct AppState {
    pub audit_recorder:      Arc<dyn AuditRecorder>,
    pub list_audit_events:   Arc<dyn ListAuditEvents>,
    pub organisations:       Arc<dyn OrganisationRepository>,
    pub roles:               Arc<dyn RoleRepository>,
    pub users:               Arc<dyn UserRepository>,
    pub tokens:              Arc<dyn TokenRepository>,
    pub jwt_config:          JwtConfig,
    pub password_reset_ttl_secs: i64,
}
```

In production, each field is wired to a concrete Postgres implementation. In tests, repositories are swapped for mocks via `MockAppStateBuilder` (see [[Testing-Strategy]]).

> [!important] No raw DB access from handlers
> Handlers never hold a `PgPool` directly. All database operations go through the repository traits on `AppState`. This enforces the abstraction boundary and makes unit testing possible without a database.

## Request Flow

```
HTTP Request
   │
   ▼
AuthLayer (tower middleware)           ← src/auth/middleware.rs
   ├─ Decode JWT → Claims
   ├─ Load permissions from DB → PermissionSet
   ├─ Check revocation table
   └─ Insert both into request extensions
   │
   ▼
Axum Router dispatch
   │
   ▼
Handler (src/<domain>/interface.rs)
   ├─ Extract Claims via AuthedCaller
   ├─ Enforce permission via Perm<P> extractor (403 if missing)
   ├─ Deserialise request body (JSON → DTO)
   └─ Call service function(state, input)
              │
              ▼
         Service (src/<domain>/service/<use_case>.rs)
              ├─ Business validation
              ├─ Repository calls (state.users.find_by_id(...))
              ├─ Emit AuditEvent (state.audit_recorder.record(...))
              └─ Return Output or Error
                        │
                        ▼
                  Repository (src/<domain>/persistence/*_pg.rs)
                        └─ sqlx query → PostgreSQL
```

## Router Assembly

[`src/lib.rs`](../../src/lib.rs) contains `build_app(cfg, pool)` which:

1. Constructs all repository implementations (wrapped in `Arc`)
2. Spawns the audit worker (mpsc channel)
3. Builds `AppState`
4. Creates two sub-routers:
   - `public` — unauthenticated routes (`/health`, `/ready`, `/api/v1/security/login`, etc.)
   - `protected` — routes wrapped in `AuthLayer`
5. Merges them, attaches CORS and tracing middleware
6. Returns `(Router, AuditWorkerHandle)`

## Module Map

```
src/
├─ main.rs                    CLI entry: serve / seed-admin / dump-openapi
├─ lib.rs                     build_app() — router assembly
├─ config.rs                  AppConfig — all EGRAS_* env vars
├─ app_state.rs               AppState — DI container
├─ db.rs                      build_pool(), run_migrations()
├─ errors.rs                  AppError → RFC 7807 JSON
├─ openapi.rs                 OpenAPI schema
├─ pagination.rs              Cursor codec (base64url-JSON)
│
├─ auth/
│   ├─ jwt.rs                 Claims, encode/decode, JwtConfig
│   ├─ middleware.rs          AuthLayer, PermissionLoader, RevocationChecker
│   ├─ permissions.rs         PermissionSet, Permission trait
│   └─ extractors.rs          AuthedCaller, Perm<P>
│
├─ security/
│   ├─ interface.rs           All security HTTP handlers + DTOs
│   ├─ model.rs               User, UserMembership, PasswordResetToken, UserCursor
│   ├─ service/
│   │   ├─ login.rs
│   │   ├─ register_user.rs
│   │   ├─ logout.rs
│   │   ├─ change_password.rs
│   │   ├─ switch_org.rs
│   │   ├─ password_reset_request.rs
│   │   ├─ password_reset_confirm.rs
│   │   ├─ list_users.rs
│   │   ├─ password_hash.rs   hash / verify / needs_rehash
│   │   └─ bootstrap_seed_admin.rs  (CLI only)
│   └─ persistence/
│       ├─ user_repository.rs       UserRepository trait
│       ├─ user_repository_pg.rs    sqlx impl
│       ├─ token_repository.rs      TokenRepository trait
│       └─ token_repository_pg.rs   sqlx impl
│
├─ tenants/
│   ├─ interface.rs
│   ├─ model.rs               Organisation, Role, Membership, OrganisationSummary, MemberSummary
│   ├─ service/
│   │   ├─ create_organisation.rs
│   │   ├─ add_user_to_organisation.rs
│   │   ├─ remove_user_from_organisation.rs
│   │   ├─ list_my_organisations.rs
│   │   ├─ list_organisation_members.rs
│   │   └─ assign_role.rs
│   └─ persistence/
│       ├─ organisation_repository.rs
│       ├─ organisation_repository_pg.rs
│       ├─ role_repository.rs
│       └─ role_repository_pg.rs
│
├─ features/
│   ├─ mod.rs
│   ├─ model.rs               FeatureDefinition, OrgFeatureOverride, EvaluatedFeature, FeatureValueType, FeatureSource
│   ├─ service/
│   │   └─ evaluate.rs        FeatureEvaluator trait + PgFeatureEvaluator (TTL cache, invalidate/invalidate_all)
│   └─ persistence/
│       ├─ feature_repository.rs    FeatureRepository trait + FeatureRepoError
│       └─ feature_repository_pg.rs FeaturePgRepository — sqlx impl (upsert CTE, FK→UnknownSlug)
│
├─ outbox/
│   ├─ model.rs               OutboxEvent, AppendRequest
│   ├─ relayer.rs             OutboxAppender, OutboxRelayer, OutboxRelayerConfig
│   └─ persistence/
│       ├─ mod.rs                  OutboxRepository trait
│       └─ outbox_repository_pg.rs sqlx impl
│
└─ audit/
    ├─ model.rs               AuditEvent, AuditCategory, Outcome, constructors
    ├─ worker.rs              AuditWorker — drains mpsc, retries DB writes
    ├─ service/
    │   ├─ record_event.rs    AuditRecorder trait + ChannelAuditRecorder
    │   └─ list_audit_events.rs  ListAuditEvents trait + impl
    └─ persistence/
        ├─ audit_repository.rs
        └─ audit_repository_pg.rs
```

## Related notes

- [[Data-Model]] — database schema that backs this architecture
- [[Authentication]] — how `AuthLayer` works in detail
- [[Authorization]] — how `Perm<P>` extractors enforce permissions
- [[Testing-Strategy]] — how the layered architecture enables isolated testing
- [[Developer-Guide]] — how to add a new use case to this structure
