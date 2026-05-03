---
title: User Deactivation & Deletion
tags:
  - future-enhancement
  - users
  - lifecycle
  - gdpr
---

# User Deactivation & Deletion

Soft-deactivate (login blocked, data retained) and hard-delete (GDPR erasure with audit tombstone).

**Scope:** State machine: active → deactivated → deleted. Cascade rules per domain. Reactivation path.

**Touches:** [[Security-Domain]], [[Audit-System]], [[GDPR-DSAR]].
