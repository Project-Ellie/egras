---
title: Error Deduplication & Alert Flood Control
tags:
  - future-enhancement
  - ops
  - observability
  - errors
---

# Error Deduplication & Alert Flood Control

Recurring errors must produce *one* signal, not thousands. A failing dependency at 1k req/s should page once, then update — not bury the on-call in identical alerts.

**Mechanism:**
1. **Fingerprint** every error: stable hash over (error slug, normalised stack frames, route, error class). Hash ignores volatile data (request ids, timestamps, user-supplied strings).
2. **Group** occurrences under the fingerprint with first-seen, last-seen, count, affected-tenants, affected-users.
3. **Notify on transitions, not occurrences:** new fingerprint → page; reopened (resolved → seen again) → page; rate-spike on existing fingerprint → page; otherwise increment counters silently.
4. **Resolution lifecycle:** open → acknowledged → resolved → (auto-reopen on recurrence). Owner assignable.
5. **Per-tenant view:** an org admin sees errors affecting *their* org only; operator sees all.

**Build vs. buy:** Sentry / GlitchTip / Highlight already do this well — wrapping one of them is the cheap path. The case for building: fingerprints can be linked to our [[Audit-System]] and per-tenant ACLs natively, and we control PII redaction.

**Integration points:**
- [[Error-Handling]]: every `AppError` already has a stable slug — make it the primary fingerprint axis.
- [[Distributed-Tracing]]: attach trace id to each occurrence for one-click drill-in.
- [[Notification-Channels]]: alerts route through the same fan-out as user notifications.
- [[Jobs]]: job failures fingerprint identically to HTTP errors.

**Why it matters for B2B:** alert fatigue is the leading cause of missed real incidents. A seed that ships with this discipline pre-wired is a meaningful differentiator.

**Touches:** [[Error-Handling]], [[Distributed-Tracing]], [[Metrics-and-SLOs]], [[Notification-Channels]], [[Audit-System]].
