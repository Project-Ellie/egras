---
title: Webhook Subscriptions
tags:
  - future-enhancement
  - integration
  - webhooks
---

# Webhook Subscriptions

Customers subscribe URLs to event types; we deliver HMAC-signed JSON with retry+DLQ.

**Scope:** Subscription CRUD per org, secret rotation, exponential backoff, replay endpoint, signature-verification docs.

**Touches:** pairs with [[Outbox]] for reliability.
