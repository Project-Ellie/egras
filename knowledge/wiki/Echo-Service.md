---
title: Echo Service
tags:
  - smoke-test
  - testing
  - auth
---

# Echo Service

Trivial endpoint that reflects the caller's identity and request payload back as JSON. Exists so end-to-end tests (notebooks, smoke checks, health probes) can exercise the full request path — auth middleware, permission enforcement, header-allowlist enforcement — without needing a real domain feature to call into.

## Why

Every other handler in egras is gated by a domain-specific permission and depends on persistence. Echo has zero side effects and one permission (`echo:invoke`), making it the right target for:

- The Python notebook harness (see [[Notebook-Harness]]) — first scenario verifies API-key auth round-trip with payload serialisation.
- Manual `curl` checks against a deployed instance.
- Future regression tests for cross-cutting middleware behaviour (header allowlist, rate limiting, observability) that don't want a domain dependency.

Code lives at `src/echo/` — only `service.rs` (pure `build_echo` function) and `interface.rs` (handlers + router). No persistence, no migrations beyond the permission seed.

## HTTP Surface

| Method | Path           | Permission     | Body                | Returns                                           |
|--------|----------------|----------------|---------------------|---------------------------------------------------|
| GET    | `/api/v1/echo` | `echo:invoke`  | —                   | `EchoResponse` with `payload: null`               |
| POST   | `/api/v1/echo` | `echo:invoke`  | any JSON (or empty) | `EchoResponse` with `payload` set to request body |

`EchoResponse`:

```json
{
  "method": "POST",
  "payload": { "hello": "world" },
  "org_id": "<uuid>",
  "key_id": "<uuid>",
  "principal_user_id": "<uuid>",
  "received_at": "2026-05-04T18:23:01Z"
}
```

`key_id` is present (and equals the API key id) when the caller authenticated via API key. It is absent for JWT-authenticated human callers — `principal_user_id` is the authoritative caller identifier in both cases.

## Permission

`echo:invoke` is seeded by migration `0013_echo_permission.sql` with deterministic UUID `00000000-0000-0000-0000-000000000501`. **Not granted to any role by default.** To allow a service account to invoke echo, either:

- Mint the API key with `scopes: ["echo:invoke"]` (per-key restriction, see [[Service-Accounts]]), AND ensure the service account's user has the permission via role assignment; OR
- Insert into `role_permissions` for whichever role the service account holds.

## Auth model

Both authentication transports work, subject to the per-org `auth.api_key_headers` allowlist (see [[Feature-Flags]]):

- **API key** via `X-API-Key: egras_<token>` or `Authorization: Bearer egras_<token>`.
- **JWT** via `Authorization: Bearer <jwt>` (a logged-in user with `echo:invoke` granted).

Cross-tenant guard is unnecessary — the response is keyed to the caller's own `org_id`, with no resource lookup.

## Cross-references

- [[Service-Accounts]] — minting API keys, scopes.
- [[Feature-Flags]] — `auth.api_key_headers` allowlist (gates which header may carry the key for a given org).
- [[Authentication]] — middleware order, header precedence.
- [[Notebook-Harness]] — first scenario (`01_echo_smoke.ipynb`) consumes this endpoint end-to-end.
