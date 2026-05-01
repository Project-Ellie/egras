---
title: Error Handling
tags:
  - errors
  - rfc7807
  - api
---

# Error Handling

egras uses a single `AppError` enum for all application errors, which is automatically serialised to [RFC 7807 Problem JSON](https://www.rfc-editor.org/rfc/rfc7807) via an `IntoResponse` implementation.

All error logic lives in [`src/errors.rs`](../../src/errors.rs).

## AppError

```rust
pub enum AppError {
    Validation {
        errors: HashMap<String, Vec<String>>,
    },
    Unauthenticated { reason: String },
    InvalidCredentials,
    PermissionDenied { code: String },
    NotFound { resource: String },
    Conflict { reason: String },
    UserNoOrganisation,
    Internal(#[from] anyhow::Error),
}
```

`AppError` implements Axum's `IntoResponse`, so any handler returning `Result<_, AppError>` automatically produces the correct HTTP response.

## ErrorSlug

Each `AppError` variant maps to a stable `ErrorSlug` string that appears in the response `type` URI. These are part of the API contract — **do not change slugs** once deployed, as clients may key on them.

| Variant | Slug | HTTP Status |
|---------|------|------------|
| `Validation` | `validation.invalid_request` | 400 |
| `Unauthenticated` | `auth.unauthenticated` | 401 |
| `InvalidCredentials` | `auth.invalid_credentials` | 401 |
| `PermissionDenied` | `permission.denied` | 403 |
| `UserNoOrganisation` | `user.no_organisation` | 403 |
| `NotFound` | `resource.not_found` | 404 |
| `Conflict` | `resource.conflict` | 409 |
| `Internal` | `internal.error` | 500 |

## RFC 7807 Response Format

Every error response has `Content-Type: application/problem+json` and this body shape:

```json
{
  "type":       "https://egras.dev/errors/auth.invalid_credentials",
  "title":      "Invalid credentials",
  "status":     401,
  "detail":     "Invalid username or password.",
  "instance":   null,
  "request_id": null,
  "errors":     null
}
```

For validation errors (400), `errors` is populated with field-level detail:

```json
{
  "type":   "https://egras.dev/errors/validation.invalid_request",
  "title":  "Invalid request",
  "status": 400,
  "detail": "One or more fields failed validation.",
  "errors": {
    "email":    ["invalid"],
    "password": ["too_short", "too_long"]
  }
}
```

The `errors` map keys are field names; values are lists of slug strings (not human prose) so clients can localise error messages.

## Internal Errors

`AppError::Internal(anyhow::Error)` is the catch-all. When it fires:

1. The full error chain is logged at `ERROR` level via `tracing`
2. The response only contains a generic `"An internal error occurred."` message — the internal detail is never leaked to the client

The `#[from] anyhow::Error` derive means any `anyhow::Error` can be converted with `?`:

```rust
let user = state.users.find_by_id(id).await
    .map_err(anyhow::Error::from)?;  // or .map_err(AppError::Internal)?;
```

## Field Errors (Validation)

The `field_error` helper in `src/security/interface.rs` and `src/tenants/interface.rs` builds validation-style errors from service-level results:

```rust
fn field_error(field: &str, code: &str) -> AppError {
    AppError::Validation {
        errors: [(field.to_string(), vec![code.to_string()])].into(),
    }
}
```

Example — mapping a service error to an HTTP response:

```rust
RegisterUserError::InvalidEmail
    => field_error("email", "invalid"),
RegisterUserError::PasswordTooShort
    => field_error("password", "too_short"),
RegisterUserError::PasswordTooLong
    => field_error("password", "too_long"),
RegisterUserError::DuplicateUsername
    => AppError::Conflict { reason: "username already taken".into() },
```

## Service vs AppError separation

Service functions return domain-specific error types (e.g., `LoginError`, `RegisterUserError`), not `AppError`. This keeps the service layer independent of HTTP concerns. The interface layer (handlers) maps service errors to `AppError` using a `match`.

```
Service:    Result<LoginOutput, LoginError>
                              ↓ (match in interface.rs)
Handler:    Result<impl IntoResponse, AppError>
```

## Related notes

- [[Architecture]] — where `AppError` sits in the request flow
- [[Security-Domain]] — service error types for security use cases
- [[Tenants-Domain]] — service error types for tenants use cases
