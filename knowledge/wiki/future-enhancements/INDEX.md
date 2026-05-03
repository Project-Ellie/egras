---
title: Future Enhancements — Index
tags:
  - future-enhancement
  - index
---

# Future Enhancements

Catalogue of features that an enterprise-grade B2B application seed *could* ship. Not a roadmap. Each note states scope, rationale, and links to existing wiki where it touches built code.

Goal: Wolfgang picks what matters; we then promote selected notes into specs under `knowledge/specs/` and plans.

## Categories

### Identity, Access & Sessions
- [[SSO-OIDC-SAML]] — federated login for enterprise customers
- [[MFA]] — TOTP + WebAuthn second factor
- [[SCIM-Provisioning]] — IdP-driven user lifecycle
- [[Service-Accounts-and-API-Keys]] — non-human principals
- [[Session-Management-UI]] — list/revoke active sessions
- [[Password-Policy-and-Breach-Check]] — strength rules + HIBP
- [[Policy-Engine-UI]] — role/permission/ABAC editor (the one you named)
- [[Delegated-Admin]] — org-scoped admin without operator

### User & Org Lifecycle
- [[User-Onboarding]] — invite, email verification, first-login
- [[User-Deactivation-and-Deletion]] — soft/hard, GDPR erasure
- [[Organization-Hierarchy]] — parent/child orgs, inherited policy
- [[Org-Onboarding-Workflow]] — provisioning state machine

### Messaging & Notifications
- [[User-and-Org-Inbox]] — durable in-app messages (the one you named)
- [[Notification-Channels]] — email/webhook/in-app fan-out
- [[Transactional-Email-Templates]] — versioned, localised
- [[Webhook-Subscriptions]] — HMAC-signed, retried, replayable

### API Management
- [[API-Management-Platform]] — umbrella tying the items below into one product
- [[Rate-Limiting-and-Quotas]] — per-org, per-key
- [[Idempotency-Keys]] — safe retries on POSTs
- [[API-Versioning-and-Deprecation]] — sunset headers, version policy
- [[Public-API-Portal]] — hosted OpenAPI explorer + key self-service
- [[API-Usage-Analytics]] — per-consumer call stats, latency, errors, deprecation visibility

### Operations & Reliability
- ~~Background-Jobs~~ — **shipped**, see [[Jobs]]
- ~~Outbox-Pattern~~ — **shipped**, see [[Outbox]]
- [[Distributed-Tracing]] — OpenTelemetry end-to-end
- [[Metrics-and-SLOs]] — Prometheus + golden signals
- [[Error-Deduplication-and-Alerting]] — fingerprinted errors, one alert per incident not thousands
- [[Backup-and-Disaster-Recovery]] — RPO/RTO runbook

### Compliance & Data
- [[GDPR-DSAR]] — export/erase per subject
- [[Data-Retention-Policies]] — per-resource TTL + legal hold
- [[Consent-and-Terms-Ledger]] — versioned acceptance log
- [[Field-Level-Encryption]] — PII tagging + envelope encryption
- [[Data-Residency]] — region pinning per tenant

### Business & Admin
- [[Billing-and-Metering]] — usage capture hooks (no billing engine)
- [[Feature-Flags]] — per-org/per-user toggles
- [[Bulk-Import-Export]] — CSV/JSON ingress + egress jobs
- [[File-Attachments]] — object-storage abstraction
- [[Search-Abstraction]] — full-text + faceted query layer
- [[Internationalization]] — i18n/l10n strategy
- [[Admin-Back-Office]] — operator console for support
- [[Support-Impersonation]] — auditable "view as" with consent
- [[Status-Page-Hooks]] — incident comms feed
