---
title: Service Accounts & API Keys
tags:
  - future-enhancement
  - auth
  - api-keys
---

# Service Accounts & API Keys

Non-human principals for server-to-server integrations.

**Scope:** Service-account user type with scoped API keys (prefix + secret), per-key permissions, rotation, last-used, revocation. Hashed at rest.

**Touches:** [[Authentication]], [[Authorization]]. Distinct from session JWTs.
