"""Wrappers for ``/api/v1/echo`` — used by smoke tests and notebooks."""

from __future__ import annotations

from typing import Any, Mapping, Optional

from ..models import EchoResponse
from ._base import _Api


class EchoApi(_Api):
    """Authenticated round-trip endpoint that echoes back principal + payload.

    Useful in scenarios to verify that an auth token (JWT or API key) is
    accepted and to confirm the active org seen by the server.
    """

    def echo_get(self) -> EchoResponse:
        """``get_echo`` — GET /api/v1/echo. Payload is always ``None`` for GET."""
        return self._c.request("GET", "/api/v1/echo", response_model=EchoResponse)

    def echo_post(self, payload: Optional[Mapping[str, Any]] = None) -> EchoResponse:
        """``post_echo`` — POST /api/v1/echo.

        The server echoes back ``payload`` verbatim. The OpenAPI schema for
        the request body is open (``{}``), so any JSON-serialisable dict
        is accepted.
        """
        return self._c.request(
            "POST", "/api/v1/echo",
            json=payload if payload is not None else {},
            response_model=EchoResponse,
        )
