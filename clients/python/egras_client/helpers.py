"""High-level scenario helpers used by the notebook examples.

These compose the lower-level :class:`Client` API into one-call shortcuts
for common multi-step flows (login as operator, bootstrap an org with a
service account and an API key, etc.). They exist purely for ergonomics —
each is a thin wrapper over typed endpoint methods.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Optional
from uuid import UUID

from .client import Client
from .models import (
    AddUserToOrganisationRequest,
    CreateApiKeyRequest,
    CreateOrganisationRequest,
    CreateServiceAccountRequest,
    LoginRequest,
)


# --- credential helpers --------------------------------------------------

def operator_credentials() -> tuple[str, str]:
    """Read the seed-operator credentials from the environment.

    Returns ``(email, password)``. Defaults match what the server's
    ``seed-admin`` CLI subcommand creates in a fresh local stack
    (``admin@smurve.ch`` / ``12345``). Override per-environment via
    ``EGRAS_OPERATOR_EMAIL`` / ``EGRAS_OPERATOR_PASSWORD``.
    """
    return (
        os.environ.get("EGRAS_OPERATOR_EMAIL", "admin@smurve.ch"),
        os.environ.get("EGRAS_OPERATOR_PASSWORD", "12345"),
    )


def login_operator(base_url: str, email: str, password: str) -> str:
    """One-shot login that returns just the JWT.

    Uses a temporary :class:`Client`. Prefer :meth:`Client.login` when you
    already have a client instance you want to keep using.
    """
    with Client(base_url) as c:
        resp = c.security.login(LoginRequest(username_or_email=email, password=password))
        return resp.token


# --- bootstrap a tenant --------------------------------------------------

@dataclass
class BootstrappedOrg:
    """Result of :func:`bootstrap_org_with_service_account`.

    ``api_key_plaintext`` is only populated on creation — the server never
    returns it again, so the caller must persist it (or rotate to obtain a
    new plaintext later).
    """

    org_id: UUID
    org_name: str
    service_account_user_id: UUID
    api_key_plaintext: str


def bootstrap_org_with_service_account(
    operator: Client,
    *,
    org_name: str,
    business: str = "Technology",
    service_account_name: str = "scenario-sa",
    service_account_role_code: str = "owner",
    api_key_name: str = "scenario-key",
    scopes: Optional[list[str]] = None,
) -> BootstrappedOrg:
    """Create org → service account → membership → API key, in one call.

    Steps performed against the live server (each step authenticated as
    ``operator``):

    1. ``POST /tenants/organisations`` to create the org.
    2. ``POST /security/service-accounts`` to create a service-account user
       in that org.
    3. ``POST /tenants/add-user-to-organisation`` to grant the SA a
       membership with ``service_account_role_code`` (``owner`` by default).
    4. ``POST /security/service-accounts/{sa_id}/api-keys`` to mint a key.

    The returned plaintext is the **only** time the key is visible. Persist
    it on the caller side; the server stores only a hash.

    Notes
    -----
    Step 3 is needed because freshly-created service accounts have no
    membership row in the org they belong to (the server-side
    ``organisation_id`` field on the SA record is metadata, not a grant).
    Without step 3, the SA would authenticate but get 403 on every org-scoped
    endpoint.
    """
    org = operator.tenants.create_organisation(
        CreateOrganisationRequest(name=org_name, business=business)
    )
    sa = operator.security.create_service_account(
        CreateServiceAccountRequest(name=service_account_name, organisation_id=org.id)
    )
    operator.tenants.add_user_to_organisation(
        AddUserToOrganisationRequest(
            org_id=org.id, user_id=sa.user_id, role_code=service_account_role_code
        )
    )
    key_resp = operator.security.create_api_key(
        sa.user_id, CreateApiKeyRequest(name=api_key_name, scopes=scopes)
    )
    return BootstrappedOrg(
        org_id=org.id,
        org_name=org.name,
        service_account_user_id=sa.user_id,
        api_key_plaintext=key_resp.plaintext,
    )
