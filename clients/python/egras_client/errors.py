"""Exception hierarchy and RFC 7807 problem-body parsing.

The egras server returns errors as application/problem+json (or, in some
older paths, application/json with the same shape). The body has the fields:

    { type, title, status, detail, instance, slug }

We model that as `ProblemBody` and raise a `status`-specific subclass of
`ApiError` so callers can `except Unauthorized` instead of branching on a
status code. The original `httpx.Response` is also kept for debugging.
"""

from __future__ import annotations

from typing import Any, Optional

import httpx
from pydantic import BaseModel, ConfigDict


class ProblemBody(BaseModel):
    """RFC 7807 problem details body.

    `slug` is an egras-specific extension used to identify the error class
    in a stable, machine-readable way (the API contract). All other fields
    are standard RFC 7807.
    """

    model_config = ConfigDict(extra="allow")

    type: Optional[str] = None
    title: Optional[str] = None
    status: Optional[int] = None
    detail: Optional[str] = None
    instance: Optional[str] = None
    slug: Optional[str] = None


class ApiError(Exception):
    """Base class for any non-2xx response from the egras API.

    Always carries the parsed `ProblemBody` (best effort) and the original
    `httpx.Response`. Subclasses exist for the common HTTP status codes so
    callers can catch them by name.
    """

    status_code: int = 0  # overridden in subclasses where the code is fixed

    def __init__(
        self,
        message: str,
        *,
        response: httpx.Response,
        problem: Optional[ProblemBody] = None,
    ) -> None:
        super().__init__(message)
        self.response = response
        self.problem = problem or ProblemBody()
        # Use the runtime status from the response; subclass `status_code`
        # attribute is just a hint for `from_response` dispatch.
        self.status_code = response.status_code

    @property
    def slug(self) -> Optional[str]:
        return self.problem.slug

    @classmethod
    def from_response(cls, response: httpx.Response) -> "ApiError":
        """Parse `response` into the most specific subclass we can.

        Falls back to `ApiError` for unmapped status codes. The body is
        parsed leniently — a non-JSON body still produces an exception with
        an empty `ProblemBody` rather than crashing.
        """
        problem = _parse_problem(response)
        message = _format_message(response, problem)
        subclass = _exc_for_status(response.status_code)
        return subclass(message, response=response, problem=problem)


# --- specific subclasses -------------------------------------------------

class BadRequest(ApiError):
    status_code = 400


class Unauthorized(ApiError):
    status_code = 401


class Forbidden(ApiError):
    status_code = 403


class NotFound(ApiError):
    status_code = 404


class Conflict(ApiError):
    status_code = 409


class UnprocessableEntity(ApiError):
    status_code = 422


class ServerError(ApiError):
    """5xx responses. The exact status is on `self.status_code`."""

    status_code = 500


_STATUS_TO_EXC: dict[int, type[ApiError]] = {
    400: BadRequest,
    401: Unauthorized,
    403: Forbidden,
    404: NotFound,
    409: Conflict,
    422: UnprocessableEntity,
}


def _parse_problem(response: httpx.Response) -> ProblemBody:
    try:
        data: Any = response.json()
    except (ValueError, httpx.DecodingError):
        return ProblemBody()
    if not isinstance(data, dict):
        return ProblemBody()
    try:
        return ProblemBody.model_validate(data)
    except Exception:
        return ProblemBody()


def _format_message(response: httpx.Response, problem: ProblemBody) -> str:
    title = problem.title or response.reason_phrase or "API error"
    parts = [f"{response.status_code} {title}"]
    if problem.slug:
        parts.append(f"[{problem.slug}]")
    if problem.detail:
        parts.append(f"- {problem.detail}")
    return " ".join(parts)


# --- 5xx mapping ---------------------------------------------------------

def _exc_for_status(status: int) -> type[ApiError]:
    if status in _STATUS_TO_EXC:
        return _STATUS_TO_EXC[status]
    if 500 <= status < 600:
        return ServerError
    return ApiError
