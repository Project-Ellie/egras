---
title: egras — Developer Wiki
tags:
  - wiki
  - overview
aliases:
  - Start Here
  - Index
---

# egras — Enterprise-Ready Rust Application Seed

**egras** is a production-shaped foundation for multi-tenant REST API backends. It combines Axum, PostgreSQL, and JWT into a complete, opinionated starting point that demonstrates how an enterprise-grade Rust service should be structured.

> [!tip] New here?
> Start with [[Architecture]] to understand how the codebase is organised, then follow the links to whichever area you're working in.

## What egras provides out of the box

- **Multi-tenancy** — organisations with per-user role assignments
- **Authentication** — JWT login/logout with revocation, password reset, org switching
- **RBAC** — role-based permissions loaded per request, enforced via type-safe Axum extractors
- **Audit trail** — immutable append-only event log with async worker, retry, and querying
- **RFC 7807 errors** — consistent problem JSON on every error response
- **OpenAPI docs** — auto-generated Swagger UI at `/swagger-ui`
- **CLI** — `serve`, `seed-admin`, `dump-openapi` subcommands
- **CI pipeline** — fmt, clippy, test, release build, OpenAPI drift check

## Wiki Map

| Note | What you'll learn |
|------|------------------|
| [[Architecture]] | 2D domain/layer design, module layout, dependency injection pattern |
| [[Data-Model]] | Database schema, migrations, entity relationships, permission matrix |
| [[Authentication]] | JWT structure, login/logout/password-reset flows, claims, middleware |
| [[Authorization]] | RBAC, permission extractors, org-scoping rules, operator bypass |
| [[Audit-System]] | Event model, async worker, categories, cursor-paginated query API |
| [[Error-Handling]] | `AppError` enum, RFC 7807 response format, error slugs |
| [[Configuration]] | All `EGRAS_*` env vars, validation, startup checks |
| [[Security-Domain]] | Users, passwords, all security use cases and their service functions |
| [[Tenants-Domain]] | Organisations, memberships, role assignment, inbound channels |
| [[Pagination]] | Opaque cursor codec, how pagination works across domains |
| [[Testing-Strategy]] | Three test layers, `TestPool`, `TestApp`, fixtures, patterns |
| [[Developer-Guide]] | Step-by-step: add a use case, add a permission, run locally |
| [[CI-and-Deployment]] | GitHub Actions workflow, Docker Compose, OpenAPI drift check |
| [[Design-Decisions]] | Why we chose this approach — trade-offs and alternatives considered |

## Quick orientation

```
src/
├─ auth/          JWT middleware, permission loading, extractors
├─ security/      Users, login, registration, password management
├─ tenants/       Organisations, memberships, role assignment
├─ audit/         Immutable event log + async writer
├─ config.rs      All env-var configuration
├─ app_state.rs   Dependency injection container
├─ errors.rs      AppError → RFC 7807 JSON
└─ lib.rs         Router assembly (build_app)

migrations/       0001..0007 — schema + seed data
tests/            Integration tests (persistence / service / HTTP / e2e)
docs/openapi.json Committed OpenAPI spec (drift-checked in CI)
```

## Tech stack at a glance

| Concern | Library |
|---------|---------|
| HTTP | Axum 0.7 + tower-http |
| Database | sqlx 0.8 + PostgreSQL 16 |
| Auth | jsonwebtoken 9 (HS256) |
| Password hashing | argon2 0.5 (Argon2id) |
| Config | figment 0.10 |
| Logging | tracing + tracing-subscriber |
| IDs | uuid v7 (time-ordered) |
| OpenAPI | utoipa 4.x + utoipa-swagger-ui |
| Testing | mockall, reqwest, serial_test |

See [[Architecture]] for the full dependency rationale.
