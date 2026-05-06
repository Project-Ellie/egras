"""HTTP client for the egras API.

`Client` is a thin, sync wrapper around `httpx.Client` that:

  * adds either a JWT bearer header or an `x-api-key` header to every request
    (mutually exclusive — pass exactly one, or neither for unauthenticated);
  * accepts dicts or Pydantic models as request bodies and serialises them
    consistently (`model_dump(mode="json", by_alias=True, exclude_none=True)`);
  * decodes JSON responses and parses them into a caller-supplied Pydantic
    model class (or returns raw dicts if no model is given);
  * maps any non-2xx response to a typed `ApiError` subclass that carries the
    parsed RFC 7807 problem body.

The endpoint methods themselves live in `egras_client.api.*`, organised by
OpenAPI tag. They are exposed as properties on `Client` so callers write
`client.tenants.create_organisation(...)`.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional, Type, TypeVar, Union

import httpx
from pydantic import BaseModel

from .errors import ApiError

T = TypeVar("T", bound=BaseModel)
JsonBody = Union[BaseModel, Mapping[str, Any], list, None]


# Module-level default; lazy-imported in __init__ to avoid a circular import.
DEFAULT_TIMEOUT = httpx.Timeout(10.0, connect=5.0)


class Client:
    """Sync egras API client.

    Parameters
    ----------
    base_url
        Server root, e.g. ``"http://localhost:8080"``. A trailing slash is
        stripped. All endpoint paths are absolute (``/api/v1/...``) and are
        appended to this.
    jwt
        JWT bearer token to send as ``Authorization: Bearer <jwt>``.
    api_key
        Plaintext service-account API key. Sent as ``x-api-key``.
    timeout
        httpx timeout. Default is 10s overall, 5s connect.
    transport
        Optional ``httpx.BaseTransport`` (handy for tests using ``MockTransport``).

    Notes
    -----
    Pass at most one of ``jwt``/``api_key``. Both are accepted for symmetry
    with the existing notebook client; the server only honours one auth
    scheme per request, but it doesn't reject a request that carries both —
    it simply prefers the bearer.
    """

    def __init__(
        self,
        base_url: str,
        *,
        jwt: Optional[str] = None,
        api_key: Optional[str] = None,
        timeout: Optional[httpx.Timeout] = None,
        transport: Optional[httpx.BaseTransport] = None,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self._jwt = jwt
        self._api_key = api_key
        self._http = httpx.Client(
            base_url=self.base_url,
            timeout=timeout or DEFAULT_TIMEOUT,
            transport=transport,
        )

        # Per-tag endpoint groups. Imported lazily to keep import time small
        # and to avoid a hard cycle (api modules import Client).
        from .api.security import SecurityApi
        from .api.tenants import TenantsApi
        from .api.echo import EchoApi
        from .api.features import FeaturesApi
        from .api.users import UsersApi

        self.security = SecurityApi(self)
        self.tenants = TenantsApi(self)
        self.echo = EchoApi(self)
        self.features = FeaturesApi(self)
        self.users = UsersApi(self)

    # --- auth state ------------------------------------------------------

    @property
    def jwt(self) -> Optional[str]:
        return self._jwt

    @jwt.setter
    def jwt(self, value: Optional[str]) -> None:
        """Replace the bearer token. Use after a login that returns a fresh
        token (for example after `switch_org`)."""
        self._jwt = value

    @property
    def api_key(self) -> Optional[str]:
        return self._api_key

    @api_key.setter
    def api_key(self, value: Optional[str]) -> None:
        self._api_key = value

    def login(self, username_or_email: str, password: str) -> None:
        """Convenience: POST /security/login and store the returned JWT in place.

        After this call, every subsequent request from this client carries
        the bearer header. Equivalent to ``client.jwt = client.security.login(...).token``.
        """
        from .models import LoginRequest  # local import: models may be absent before regen

        resp = self.security.login(LoginRequest(username_or_email=username_or_email, password=password))
        self._jwt = resp.token

    # --- context-manager / cleanup --------------------------------------

    def close(self) -> None:
        self._http.close()

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    # --- low-level request ----------------------------------------------

    def _headers(self, extra: Optional[Mapping[str, str]] = None) -> dict[str, str]:
        h: dict[str, str] = {"accept": "application/json"}
        if self._jwt:
            h["authorization"] = f"Bearer {self._jwt}"
        if self._api_key:
            h["x-api-key"] = self._api_key
        if extra:
            h.update(extra)
        return h

    @staticmethod
    def _serialise(body: JsonBody) -> Any:
        """Pydantic-aware JSON serialisation.

        We pass ``by_alias=True`` so models that declare aliases (e.g.
        ``Field(..., alias="organisation_id")``) round-trip correctly, and
        ``exclude_none=True`` so optional fields default to "absent" rather
        than ``null`` — that matches the server's wire contract.
        """
        if body is None:
            return None
        if isinstance(body, BaseModel):
            return body.model_dump(mode="json", by_alias=True, exclude_none=True)
        return body

    def request(
        self,
        method: str,
        path: str,
        *,
        json: JsonBody = None,
        params: Optional[Mapping[str, Any]] = None,
        headers: Optional[Mapping[str, str]] = None,
        response_model: Optional[Type[T]] = None,
        expect_no_content: bool = False,
    ) -> Any:
        """Perform a request and return parsed JSON or a Pydantic instance.

        Parameters
        ----------
        method
            HTTP method.
        path
            Server path, including the ``/api/v1/...`` prefix.
        json
            Request body. May be a Pydantic model, a dict/list, or ``None``.
        params
            Query string parameters. ``None`` values are dropped.
        response_model
            If set, the parsed JSON body is validated into this Pydantic
            class and returned. Otherwise raw JSON (typically a dict) is
            returned. Ignored when ``expect_no_content`` is True.
        expect_no_content
            If True, returns ``None`` and skips JSON parsing (for 204
            responses such as ``add_user_to_organisation``).

        Raises
        ------
        ApiError
            Or one of its status-specific subclasses (Unauthorized,
            Forbidden, ...) when the server returns 4xx/5xx.
        """
        cleaned_params = (
            {k: v for k, v in params.items() if v is not None} if params else None
        )
        response = self._http.request(
            method,
            path,
            json=self._serialise(json),
            params=cleaned_params,
            headers=self._headers(headers),
        )
        if response.status_code >= 400:
            raise ApiError.from_response(response)

        if expect_no_content or response.status_code == 204 or not response.content:
            return None

        data = response.json()
        if response_model is None:
            return data
        return response_model.model_validate(data)
