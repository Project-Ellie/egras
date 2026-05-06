"""Wrappers for ``/api/v1/tenants/*`` — organisations, members, channels."""

from __future__ import annotations

from typing import Optional, Union
from uuid import UUID

from ..models import (
    AddUserToOrganisationRequest,
    AssignRoleRequest,
    AssignRoleResponseBody,
    ChannelBody,
    CreateChannelRequest,
    CreateOrganisationRequest,
    OrganisationBody,
    PagedChannels,
    PagedMembers,
    PagedOrganisations,
    RemoveUserFromOrganisationRequest,
    UpdateChannelRequest,
)
from ._base import _Api


class TenantsApi(_Api):
    """Organisations, memberships, and channels."""

    # --- organisations -------------------------------------------------

    def list_my_organisations(
        self, *, after: Optional[str] = None, limit: Optional[int] = None,
    ) -> PagedOrganisations:
        """``get_list_my_organisations`` — GET /api/v1/tenants/me/organisations.

        Returns the orgs the authenticated principal is a member of.
        """
        return self._c.request(
            "GET", "/api/v1/tenants/me/organisations",
            params={"after": after, "limit": limit},
            response_model=PagedOrganisations,
        )

    def create_organisation(
        self, body: Union[CreateOrganisationRequest, dict]
    ) -> OrganisationBody:
        """``post_create_organisation`` — POST /api/v1/tenants/organisations.

        Defaults: ``seed_creator_as_owner=True`` server-side, so the caller
        becomes an ``owner`` of the new org unless they pass ``False``.
        """
        return self._c.request(
            "POST", "/api/v1/tenants/organisations",
            json=body, response_model=OrganisationBody,
        )

    # --- members & roles -----------------------------------------------

    def list_members(
        self,
        org_id: Union[UUID, str],
        *,
        after: Optional[str] = None,
        limit: Optional[int] = None,
    ) -> PagedMembers:
        """``get_list_members`` — GET /api/v1/tenants/organisations/{id}/members."""
        return self._c.request(
            "GET", f"/api/v1/tenants/organisations/{org_id}/members",
            params={"after": after, "limit": limit},
            response_model=PagedMembers,
        )

    def assign_role(
        self,
        org_id: Union[UUID, str],
        body: Union[AssignRoleRequest, dict],
    ) -> AssignRoleResponseBody:
        """``post_assign_role`` — POST /api/v1/tenants/organisations/{id}/memberships.

        Updates the role of an *existing* member. For brand-new users use
        :meth:`add_user_to_organisation` first.
        """
        return self._c.request(
            "POST", f"/api/v1/tenants/organisations/{org_id}/memberships",
            json=body, response_model=AssignRoleResponseBody,
        )

    def add_user_to_organisation(
        self, body: Union[AddUserToOrganisationRequest, dict]
    ) -> None:
        """``post_add_user_to_organisation`` — POST /api/v1/tenants/add-user-to-organisation.

        Grants a non-member user a membership in an org with the supplied
        ``role_code``. Returns 204.
        """
        self._c.request(
            "POST", "/api/v1/tenants/add-user-to-organisation",
            json=body, expect_no_content=True,
        )

    def remove_user_from_organisation(
        self, body: Union[RemoveUserFromOrganisationRequest, dict]
    ) -> None:
        """``post_remove_user_from_organisation`` — POST /api/v1/tenants/remove-user-from-organisation. Returns 204."""
        self._c.request(
            "POST", "/api/v1/tenants/remove-user-from-organisation",
            json=body, expect_no_content=True,
        )

    # --- channels ------------------------------------------------------

    def list_channels(
        self,
        org_id: Union[UUID, str],
        *,
        after: Optional[str] = None,
        limit: Optional[int] = None,
    ) -> PagedChannels:
        """``get_list_channels`` — GET /api/v1/tenants/organisations/{org_id}/channels."""
        return self._c.request(
            "GET", f"/api/v1/tenants/organisations/{org_id}/channels",
            params={"after": after, "limit": limit},
            response_model=PagedChannels,
        )

    def create_channel(
        self,
        org_id: Union[UUID, str],
        body: Union[CreateChannelRequest, dict],
    ) -> ChannelBody:
        """``post_create_channel`` — POST /api/v1/tenants/organisations/{org_id}/channels.

        On 201 the response body includes the channel's ``api_key`` — that
        is the only time it is returned in plaintext (same contract as
        service-account keys).
        """
        return self._c.request(
            "POST", f"/api/v1/tenants/organisations/{org_id}/channels",
            json=body, response_model=ChannelBody,
        )

    def get_channel(
        self, org_id: Union[UUID, str], channel_id: Union[UUID, str],
    ) -> ChannelBody:
        """``get_channel`` — GET .../channels/{channel_id}."""
        return self._c.request(
            "GET", f"/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
            response_model=ChannelBody,
        )

    def update_channel(
        self,
        org_id: Union[UUID, str],
        channel_id: Union[UUID, str],
        body: Union[UpdateChannelRequest, dict],
    ) -> ChannelBody:
        """``put_update_channel`` — PUT .../channels/{channel_id}.

        Note: this is a full PUT — all fields on ``UpdateChannelRequest``
        must be supplied. The ``api_key`` is *not* rotated by this call.
        """
        return self._c.request(
            "PUT", f"/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
            json=body, response_model=ChannelBody,
        )

    def delete_channel(
        self, org_id: Union[UUID, str], channel_id: Union[UUID, str],
    ) -> None:
        """``delete_channel`` — DELETE .../channels/{channel_id}. 204."""
        self._c.request(
            "DELETE", f"/api/v1/tenants/organisations/{org_id}/channels/{channel_id}",
            expect_no_content=True,
        )
