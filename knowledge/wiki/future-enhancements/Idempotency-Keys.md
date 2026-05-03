---
title: Idempotency Keys
tags:
  - future-enhancement
  - api
  - reliability
---

# Idempotency Keys

Clients send Idempotency-Key on POSTs; we replay stored response on duplicates.

**Scope:** Key→response store with TTL, scoped to endpoint+user. Stripe-style.

**Touches:** middleware; affects all mutating handlers.
