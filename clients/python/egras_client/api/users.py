"""Wrappers for ``/api/v1/users``."""

from __future__ import annotations

from typing import Optional, Union
from uuid import UUID

from ..models import ListUsersResponse
from ._base import _Api


class UsersApi(_Api):
    """User directory queries (cross-org listing for operators, in-org otherwise)."""

    def list_users(
        self,
        *,
        after: Optional[str] = None,
        limit: Optional[int] = None,
        org_id: Optional[Union[UUID, str]] = None,
        q: Optional[str] = None,
    ) -> ListUsersResponse:
        """``get_list_users`` — GET /api/v1/users.

        Parameters
        ----------
        after, limit
            Cursor pagination.
        org_id
            Restrict to members of a specific org. Required for non-operator
            principals (the server enforces).
        q
            Substring search across email/username.
        """
        return self._c.request(
            "GET", "/api/v1/users",
            params={
                "after": after,
                "limit": limit,
                "org_id": str(org_id) if org_id is not None else None,
                "q": q,
            },
            response_model=ListUsersResponse,
        )
