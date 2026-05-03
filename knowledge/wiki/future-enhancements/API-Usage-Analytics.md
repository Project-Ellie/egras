---
title: API Usage Analytics
tags:
  - future-enhancement
  - api
  - observability
  - analytics
---

# API Usage Analytics

Per-consumer (org / API key / user) request statistics for product, billing, capacity, and deprecation decisions.

**Captured per request:** route template, status class, latency bucket, request/response bytes, API version, key id, org id, user id (if any). Aggregated continuously; raw retained for a window.

**Surfaces:**
- Operator dashboard: top consumers, error-rate hot spots, p95/p99 per route, traffic trend.
- Customer dashboard (in [[Public-API-Portal]] or [[Admin-Back-Office]]): a tenant's own usage and quota burn-down.
- Export: hooks into [[Billing-and-Metering]].
- Deprecation visibility: who is still calling deprecated routes, by version.

**Storage options:** start with Postgres rollups (hourly/daily) populated by an aggregator job; promote to ClickHouse/Timescale only if cardinality forces it. Avoid emitting per-request rows into hot OLTP — write to an outbox or use a sampling middleware.

**Distinct from [[Metrics-and-SLOs]]:** Prometheus is for ops health (cardinality-bounded). This is per-tenant, per-key, retained, queryable.

**Touches:** [[API-Management-Platform]], [[Rate-Limiting-and-Quotas]], [[Billing-and-Metering]], [[Audit-System]].
