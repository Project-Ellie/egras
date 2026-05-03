---
title: SSO (OIDC / SAML)
tags:
  - future-enhancement
  - auth
  - identity
  - sso
---

# SSO (OIDC / SAML)

Federated login so enterprise customers use their own IdP (Okta, Entra, Google Workspace).

**Scope:** OIDC authorization-code flow first; SAML 2.0 second pass. Per-org IdP config (issuer, client-id, JWKS URL). Local accounts remain for operator org and break-glass.

**Touches:** [[Authentication]], [[Tenants-Domain]], [[Authorization]] (group→role mapping).

**Why maybe later:** Significant config surface; only matters once first paying enterprise asks.
