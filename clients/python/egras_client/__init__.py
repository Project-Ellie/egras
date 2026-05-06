"""egras Python client.

Public surface kept small on purpose. Most work is done through `Client` and
its per-tag attributes (`client.security`, `client.tenants`, ...).
"""

from .client import Client
from .errors import (
    ApiError,
    BadRequest,
    Conflict,
    Forbidden,
    NotFound,
    ProblemBody,
    ServerError,
    Unauthorized,
    UnprocessableEntity,
)

__all__ = [
    "Client",
    "ApiError",
    "BadRequest",
    "Unauthorized",
    "Forbidden",
    "NotFound",
    "Conflict",
    "UnprocessableEntity",
    "ServerError",
    "ProblemBody",
]

__version__ = "0.1.0"
