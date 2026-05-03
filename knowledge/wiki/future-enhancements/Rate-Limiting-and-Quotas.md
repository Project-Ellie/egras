---
title: Rate Limiting & Quotas
tags:
  - future-enhancement
  - api
  - ops
---

# Rate Limiting & Quotas

Per-org and per-key request budgets; tiered plans.

**Scope:** Token-bucket middleware backed by Redis or Postgres. Quota counters reset on schedule. 429 with Retry-After.

**Touches:** [[Error-Handling]].
