"""Thin egras HTTP client + scenario helpers. Used by Jupyter notebooks
under `notebooks/scenarios/`. Server expected at BASE_URL — caller's
responsibility to ensure it is up (see notebooks/README.md)."""

from __future__ import annotations
import os
from typing import Any, Optional
import requests


class Client:
    def __init__(self, base_url: str, *, jwt: Optional[str] = None, api_key: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        self.jwt = jwt
        self.api_key = api_key

    def _headers(self) -> dict:
        h = {"content-type": "application/json"}
        if self.jwt:
            h["authorization"] = f"Bearer {self.jwt}"
        if self.api_key:
            h["x-api-key"] = self.api_key
        return h

    def get(self, path: str, **kwargs) -> requests.Response:
        return requests.get(self.base_url + path, headers=self._headers(), **kwargs)

    def post(self, path: str, **kwargs) -> requests.Response:
        return requests.post(self.base_url + path, headers=self._headers(), **kwargs)

    def put(self, path: str, **kwargs) -> requests.Response:
        return requests.put(self.base_url + path, headers=self._headers(), **kwargs)

    def delete(self, path: str, **kwargs) -> requests.Response:
        return requests.delete(self.base_url + path, headers=self._headers(), **kwargs)


def login_operator(base_url: str, email: str, password: str) -> str:
    """Logs in via /api/v1/security/login and returns the JWT token.

    The login request uses `username_or_email`; the response field is `token`.
    """
    r = requests.post(
        base_url.rstrip("/") + "/api/v1/security/login",
        json={"username_or_email": email, "password": password},
    )
    r.raise_for_status()
    return r.json()["token"]


def create_org(client: Client, name: str, business: str = "Technology") -> dict:
    """POST /api/v1/tenants/organisations. Returns the response body (dict including id).

    Requires `name` and `business` (both min-length 1, max 120).
    seed_creator_as_owner defaults to true server-side.
    """
    r = client.post(
        "/api/v1/tenants/organisations",
        json={"name": name, "business": business},
    )
    r.raise_for_status()
    return r.json()


def create_service_account(client: Client, org_id: str, name: str) -> dict:
    """POST /api/v1/security/service-accounts. Returns the response body (includes user_id)."""
    r = client.post(
        "/api/v1/security/service-accounts",
        json={"organisation_id": org_id, "name": name, "description": None},
    )
    r.raise_for_status()
    return r.json()


def add_user_to_org(client: Client, org_id: str, user_id: str, role_code: str) -> None:
    """POST /api/v1/tenants/add-user-to-organisation.

    Creates the membership row for user_id in org_id with role_code. Use this
    to grant a freshly-created service account membership in its org (SAs are
    created as users without any membership; this step makes them members).

    Body: { user_id, org_id, role_code }. Returns 204.
    """
    r = client.post(
        "/api/v1/tenants/add-user-to-organisation",
        json={"user_id": user_id, "org_id": org_id, "role_code": role_code},
    )
    r.raise_for_status()


def assign_role(client: Client, org_id: str, user_id: str, role_code: str) -> None:
    """POST /api/v1/tenants/organisations/{org_id}/memberships.

    Updates the role of an EXISTING member. The user must already be a member
    of the org (use add_user_to_org first for fresh service accounts).

    Body: { user_id, role_code }. Returns 200 { assigned: bool }.
    """
    r = client.post(
        f"/api/v1/tenants/organisations/{org_id}/memberships",
        json={"user_id": user_id, "role_code": role_code},
    )
    r.raise_for_status()


def mint_api_key(
    client: Client,
    sa_id: str,
    name: str = "scenario-key",
    *,
    scopes: Optional[list[str]] = None,
) -> tuple[str, dict]:
    """POST /api/v1/security/service-accounts/{sa_id}/api-keys.

    Returns (plaintext, full-response-dict).

    scopes=None inherits all SA permissions; scopes=["echo:invoke"] restricts.
    Note: sa_id is a path parameter (the service account's user_id).
    Response shape: { key: { id, prefix, name, scopes, ... }, plaintext: str }.
    """
    body: dict[str, Any] = {"name": name}
    if scopes is not None:
        body["scopes"] = scopes
    r = client.post(
        f"/api/v1/security/service-accounts/{sa_id}/api-keys",
        json=body,
    )
    r.raise_for_status()
    payload = r.json()
    return payload["plaintext"], payload


def operator_credentials() -> tuple[str, str]:
    """Returns (email, password) from env vars with documented defaults."""
    return (
        os.environ.get("EGRAS_OPERATOR_EMAIL", "admin@smurve.ch"),
        os.environ.get("EGRAS_OPERATOR_PASSWORD", "12345"),
    )
