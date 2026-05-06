"""Unit tests for the client core. No live server required.

These exercise auth header injection, JSON serialisation of typed bodies,
RFC 7807 error mapping, and the Pydantic models' tolerance of unknown
fields. Live integration is covered by the notebook scenarios under
``notebooks/scenarios/``.
"""

from __future__ import annotations

import json
from uuid import UUID, uuid4

import httpx
import pytest

from egras_client import (
    ApiError,
    Client,
    Forbidden,
    NotFound,
    ProblemBody,
    Unauthorized,
)
from egras_client.models import (
    CreateOrganisationRequest,
    LoginRequest,
)


def _mock_transport(handler):
    """Wrap a request->response function as an httpx.MockTransport."""
    return httpx.MockTransport(handler)


def test_jwt_header_set_when_jwt_passed():
    seen: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["auth"] = request.headers.get("authorization")
        seen["xak"] = request.headers.get("x-api-key")
        return httpx.Response(200, json={"ok": True})

    c = Client("http://x", jwt="abc.def.ghi", transport=_mock_transport(handler))
    c.request("GET", "/api/v1/anything")
    assert seen["auth"] == "Bearer abc.def.ghi"
    assert seen["xak"] is None


def test_api_key_header_set_when_api_key_passed():
    seen: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["auth"] = request.headers.get("authorization")
        seen["xak"] = request.headers.get("x-api-key")
        return httpx.Response(200, json={})

    c = Client("http://x", api_key="plain-key", transport=_mock_transport(handler))
    c.request("GET", "/api/v1/anything")
    assert seen["auth"] is None
    assert seen["xak"] == "plain-key"


def test_typed_body_is_serialised_with_aliases_and_no_nones():
    """`exclude_none=True` so optional fields default to "absent" on the wire."""
    seen_body: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen_body.update(json.loads(request.content))
        return httpx.Response(201, json={
            "id": str(uuid4()),
            "name": "X",
            "business": "B",
            "role_codes": ["owner"],
        })

    c = Client("http://x", transport=_mock_transport(handler))
    c.tenants.create_organisation(
        CreateOrganisationRequest(name="X", business="B")  # seed_creator_as_owner left None
    )
    assert seen_body == {"name": "X", "business": "B"}


def test_login_helper_sets_jwt_in_place():
    """Client.login should mutate the bearer token used by subsequent calls."""
    state: dict = {"calls": 0, "auth_header": None}

    def handler(request: httpx.Request) -> httpx.Response:
        state["calls"] += 1
        if request.url.path.endswith("/login"):
            return httpx.Response(200, json={
                "active_org_id": str(uuid4()),
                "memberships": [],
                "token": "freshly-minted",
                "user_id": str(uuid4()),
            })
        state["auth_header"] = request.headers.get("authorization")
        return httpx.Response(200, json={
            "key_id": None,
            "method": "GET",
            "org_id": str(uuid4()),
            "payload": None,
            "principal_user_id": str(uuid4()),
            "received_at": "2026-05-06T12:00:00Z",
        })

    c = Client("http://x", transport=_mock_transport(handler))
    c.login("a@b.c", "pw")
    assert c.jwt == "freshly-minted"
    c.echo.echo_get()
    assert state["auth_header"] == "Bearer freshly-minted"


@pytest.mark.parametrize(
    "status, exc",
    [
        (401, Unauthorized),
        (403, Forbidden),
        (404, NotFound),
        (500, ApiError),  # ServerError is a subclass; ApiError catches it.
    ],
)
def test_error_status_maps_to_typed_exception(status, exc):
    body = {
        "type": "egras://errors/something.broke",
        "title": "Boom",
        "status": status,
        "detail": "details here",
        "slug": "something.broke",
    }

    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(status, json=body)

    c = Client("http://x", transport=_mock_transport(handler))
    with pytest.raises(exc) as info:
        c.echo.echo_get()
    assert isinstance(info.value, ApiError)
    assert info.value.status_code == status
    assert info.value.problem.slug == "something.broke"
    assert info.value.problem.detail == "details here"


def test_error_with_non_json_body_does_not_crash():
    """A 502 with an HTML body should still raise — with empty ProblemBody."""

    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(502, content=b"<html>upstream gone</html>")

    c = Client("http://x", transport=_mock_transport(handler))
    with pytest.raises(ApiError) as info:
        c.echo.echo_get()
    assert info.value.status_code == 502
    assert info.value.problem == ProblemBody()


def test_response_model_is_forward_compatible_with_unknown_fields():
    """Server adding a field doesn't break clients (extra='allow' on every model)."""

    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={
            "key_id": None,
            "method": "GET",
            "org_id": str(uuid4()),
            "payload": None,
            "principal_user_id": str(uuid4()),
            "received_at": "2026-05-06T12:00:00Z",
            "future_field_added_after_release": True,  # would crash without extra=allow
        })

    c = Client("http://x", transport=_mock_transport(handler))
    resp = c.echo.echo_get()
    assert resp.method == "GET"
    assert isinstance(resp.org_id, UUID)


def test_query_param_none_values_are_dropped():
    """Avoids sending `?after=None` literally."""
    seen: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["url"] = str(request.url)
        return httpx.Response(200, json={"items": [], "next_cursor": None})

    c = Client("http://x", transport=_mock_transport(handler))
    c.tenants.list_my_organisations(after=None, limit=None)
    assert "after=" not in seen["url"]
    assert "limit=" not in seen["url"]


def test_204_no_content_returns_none():
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(204)

    c = Client("http://x", jwt="t", transport=_mock_transport(handler))
    result = c.security.logout()
    assert result is None


def test_dict_body_is_accepted_in_place_of_typed_model():
    seen: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen.update(json.loads(request.content))
        return httpx.Response(200, json={
            "active_org_id": str(uuid4()),
            "memberships": [],
            "token": "tok",
            "user_id": str(uuid4()),
        })

    c = Client("http://x", transport=_mock_transport(handler))
    # dict in place of LoginRequest
    c.security.login({"username_or_email": "a@b.c", "password": "pw"})
    assert seen == {"username_or_email": "a@b.c", "password": "pw"}
