---
title: Field-Level Encryption
tags:
  - future-enhancement
  - security
  - encryption
  - pii
---

# Field-Level Encryption

Encrypt designated PII columns with envelope encryption; KMS-managed DEKs.

**Scope:** Tag fields in domain models, transparent encrypt/decrypt at repository layer, key-rotation runbook.

**Touches:** [[Data-Model]], [[Security-Domain]].

**Why maybe later:** TLS + at-rest encryption usually satisfies first-round security review.
