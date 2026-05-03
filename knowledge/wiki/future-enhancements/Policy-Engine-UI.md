---
title: Policy Engine UI (RBAC + ABAC)
tags:
  - future-enhancement
  - auth
  - rbac
  - abac
  - ui
---

# Policy Engine UI (RBAC + ABAC)

Self-service editor for roles, permissions, resource-attribute conditions. (Wolfgang named this.)

**Scope:** Role CRUD; permission catalog from code; ABAC predicates over resource attrs (e.g. resource.region == user.region). Policies versioned and audited. UI in web frontend; engine evaluates each request.

**Touches:** [[Authorization]], [[Audit-System]], [[Web-Frontend]]. Cedar/OPA-style engine, or homegrown if scope stays small.
