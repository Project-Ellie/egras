"""Wrappers for ``/api/v1/security/*`` (and the related ``/api/v1/security/service-accounts/*``).

Operation IDs from ``docs/openapi.json`` are kept verbatim in each docstring
so the drift-check script can match wrappers to spec entries.
"""

from __future__ import annotations

from typing import Optional, Union
from uuid import UUID

from ..models import (
    ChangePasswordRequest,
    CreateApiKeyRequest,
    CreateApiKeyResponse,
    CreateServiceAccountRequest,
    ListApiKeysResponse,
    ListServiceAccountsResponse,
    LoginRequest,
    LoginResponse,
    PasswordResetConfirmBody,
    PasswordResetRequestBody,
    RegisterRequest,
    RegisterResponse,
    RotateApiKeyRequest,
    ServiceAccountResponse,
    SwitchOrgRequest,
    TokenResponse,
)
from ._base import _Api

# Some endpoints accept either a typed model or a raw dict. The latter is
# convenient in notebooks; the former is convenient in typed code.
_BodyOrDict = Union[dict, "object"]


class SecurityApi(_Api):
    """Authentication, accounts, password lifecycle, service accounts, API keys."""

    # --- auth & session ------------------------------------------------

    def login(self, body: Union[LoginRequest, dict]) -> LoginResponse:
        """``post_login`` — POST /api/v1/security/login.

        Returns a fresh JWT plus the user's memberships and ``active_org_id``.
        Use :meth:`Client.login` for the common "login and remember the
        token on the client" flow.
        """
        return self._c.request(
            "POST", "/api/v1/security/login",
            json=body, response_model=LoginResponse,
        )

    def logout(self) -> None:
        """``post_logout`` — POST /api/v1/security/logout. Returns 204."""
        self._c.request("POST", "/api/v1/security/logout", expect_no_content=True)

    def switch_org(self, body: Union[SwitchOrgRequest, dict]) -> TokenResponse:
        """``post_switch_org`` — POST /api/v1/security/switch-org.

        Mints a new JWT scoped to ``org_id``. The caller must already be a
        member of the target org. Note: the *current* token is not
        invalidated server-side.
        """
        return self._c.request(
            "POST", "/api/v1/security/switch-org",
            json=body, response_model=TokenResponse,
        )

    # --- account lifecycle ---------------------------------------------

    def register(self, body: Union[RegisterRequest, dict]) -> RegisterResponse:
        """``post_register`` — POST /api/v1/security/register.

        Creates a new user and seeds their first membership. Requires a
        caller with sufficient permissions (typically operator-org admin).
        """
        return self._c.request(
            "POST", "/api/v1/security/register",
            json=body, response_model=RegisterResponse,
        )

    def change_password(self, body: Union[ChangePasswordRequest, dict]) -> None:
        """``post_change_password`` — POST /api/v1/security/change-password. Returns 204."""
        self._c.request(
            "POST", "/api/v1/security/change-password",
            json=body, expect_no_content=True,
        )

    def password_reset_request(self, body: Union[PasswordResetRequestBody, dict]) -> None:
        """``post_password_reset_request`` — POST /api/v1/security/password-reset-request.

        Always 204 (servers don't disclose whether the email is known).
        """
        self._c.request(
            "POST", "/api/v1/security/password-reset-request",
            json=body, expect_no_content=True,
        )

    def password_reset_confirm(self, body: Union[PasswordResetConfirmBody, dict]) -> None:
        """``post_password_reset_confirm`` — POST /api/v1/security/password-reset-confirm. Returns 204."""
        self._c.request(
            "POST", "/api/v1/security/password-reset-confirm",
            json=body, expect_no_content=True,
        )

    # --- service accounts ----------------------------------------------

    def list_service_accounts(
        self,
        organisation_id: Union[UUID, str],
        *,
        limit: Optional[int] = None,
        after: Optional[str] = None,
    ) -> ListServiceAccountsResponse:
        """``get_list_service_accounts`` — GET /api/v1/security/service-accounts.

        ``organisation_id`` is a required query param (servers don't
        cross-tenant by default).
        """
        return self._c.request(
            "GET", "/api/v1/security/service-accounts",
            params={"organisation_id": str(organisation_id), "limit": limit, "after": after},
            response_model=ListServiceAccountsResponse,
        )

    def create_service_account(
        self, body: Union[CreateServiceAccountRequest, dict]
    ) -> ServiceAccountResponse:
        """``post_create_service_account`` — POST /api/v1/security/service-accounts.

        The created SA has no org membership yet. Either pass
        ``seed_creator_as_owner`` semantics on the org, or follow up with
        ``add_user_to_organisation`` to grant a role.
        """
        return self._c.request(
            "POST", "/api/v1/security/service-accounts",
            json=body, response_model=ServiceAccountResponse,
        )

    def get_service_account(self, sa_id: Union[UUID, str]) -> ServiceAccountResponse:
        """``get_service_account`` — GET /api/v1/security/service-accounts/{sa_id}."""
        return self._c.request(
            "GET", f"/api/v1/security/service-accounts/{sa_id}",
            response_model=ServiceAccountResponse,
        )

    def delete_service_account(self, sa_id: Union[UUID, str]) -> None:
        """``delete_service_account_handler`` — DELETE .../service-accounts/{sa_id}. 204."""
        self._c.request(
            "DELETE", f"/api/v1/security/service-accounts/{sa_id}",
            expect_no_content=True,
        )

    # --- api keys -------------------------------------------------------

    def list_api_keys(self, sa_id: Union[UUID, str]) -> ListApiKeysResponse:
        """``get_list_api_keys`` — GET .../service-accounts/{sa_id}/api-keys."""
        return self._c.request(
            "GET", f"/api/v1/security/service-accounts/{sa_id}/api-keys",
            response_model=ListApiKeysResponse,
        )

    def create_api_key(
        self, sa_id: Union[UUID, str], body: Union[CreateApiKeyRequest, dict]
    ) -> CreateApiKeyResponse:
        """``post_create_api_key`` — POST .../service-accounts/{sa_id}/api-keys.

        The plaintext is in ``response.plaintext`` and is the **only time**
        it is returned. Persist it on the caller side; the server only
        keeps a hash.
        """
        return self._c.request(
            "POST", f"/api/v1/security/service-accounts/{sa_id}/api-keys",
            json=body, response_model=CreateApiKeyResponse,
        )

    def delete_api_key(self, sa_id: Union[UUID, str], key_id: Union[UUID, str]) -> None:
        """``delete_api_key_handler`` — DELETE .../api-keys/{key_id}. 204."""
        self._c.request(
            "DELETE", f"/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}",
            expect_no_content=True,
        )

    def rotate_api_key(
        self,
        sa_id: Union[UUID, str],
        key_id: Union[UUID, str],
        body: Union[RotateApiKeyRequest, dict, None] = None,
    ) -> CreateApiKeyResponse:
        """``post_rotate_api_key`` — POST .../api-keys/{key_id}/rotate.

        Issues a fresh plaintext. The previous key id is preserved
        server-side as revoked. Body may carry an updated ``name`` or
        ``scopes``; pass an empty body (``{}``) to keep both.
        """
        return self._c.request(
            "POST",
            f"/api/v1/security/service-accounts/{sa_id}/api-keys/{key_id}/rotate",
            json=body or {},
            response_model=CreateApiKeyResponse,
        )
