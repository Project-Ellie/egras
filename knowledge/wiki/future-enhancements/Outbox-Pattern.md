---
title: Outbox Pattern
tags:
  - future-enhancement
  - ops
  - reliability
  - events
---

# Outbox Pattern

Atomic write of domain change + event row in same transaction; relayer publishes to webhooks/queue.

**Scope:** outbox table, relayer task, at-least-once semantics, idempotent consumers.

**Touches:** pairs with [[Webhook-Subscriptions]].
