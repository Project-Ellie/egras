"""Shared base class for the per-prefix API wrappers."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:  # avoid runtime cycle: client.py imports the api modules
    from ..client import Client


class _Api:
    """Holds a back-reference to the owning :class:`Client`.

    Subclasses delegate every HTTP call through ``self._c.request(...)``
    so retries, auth, error mapping and response parsing live in one place.
    """

    def __init__(self, client: "Client") -> None:
        self._c = client
