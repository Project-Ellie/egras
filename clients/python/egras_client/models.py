"""Pydantic v2 models mirroring the egras OpenAPI schema.

AUTO-GENERATED — do not edit by hand. The source of truth is
``docs/openapi.json``; regenerate via:

    python clients/python/scripts/regen.py
"""

from __future__ import annotations

from enum import Enum
from typing import Any
from uuid import UUID

from pydantic import AwareDatetime, BaseModel, ConfigDict, Field


class AddUserToOrganisationRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    org_id: UUID
    role_code: str
    user_id: UUID


class ApiKeyResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    created_at: AwareDatetime
    id: UUID
    last_used_at: AwareDatetime | None = None
    name: str
    prefix: str
    revoked_at: AwareDatetime | None = None
    scopes: list[str] | None = None


class AssignRoleRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    role_code: str
    user_id: UUID


class AssignRoleResponseBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    assigned: bool


class ChangePasswordRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    current_password: str
    new_password: str


class ChannelType(Enum):
    vast = "vast"
    sensor = "sensor"
    websocket = "websocket"
    rest = "rest"


class CreateApiKeyRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    name: str
    scopes: list[str] | None = Field(
        None,
        description="`null` = inherit all of the service account's permissions.\nEmpty array is rejected.",
    )


class CreateApiKeyResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    key: ApiKeyResponse
    plaintext: str = Field(
        ...,
        description="Plaintext token. Returned exactly once at creation time; the server keeps\nonly the argon2 hash. Show it to the operator immediately.",
    )


class CreateChannelRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    channel_type: ChannelType
    description: str | None = None
    is_active: bool | None = None
    name: str


class CreateOrganisationRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    business: str
    name: str
    seed_creator_as_owner: bool | None = None


class CreateServiceAccountRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    description: str | None = None
    name: str
    organisation_id: UUID


class EchoResponse(BaseModel):
    """
    Response body returned by both GET and POST /api/v1/echo.
    """

    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    key_id: UUID | None = Field(
        None,
        description="API key ID when authenticated via API key; `null` for JWT callers.",
    )
    method: str = Field(
        ..., description='HTTP method of the incoming request ("GET" or "POST").'
    )
    org_id: UUID = Field(..., description="Organisation the caller belongs to.")
    payload: Any
    principal_user_id: UUID = Field(
        ..., description="The principal user ID (human user or service account user)."
    )
    received_at: AwareDatetime = Field(
        ...,
        description="Server-side timestamp at which the request was received (RFC 3339).",
    )


class ErrorBody(BaseModel):
    """
    RFC 7807 problem body returned on all error responses.

    All six stable fields are present in every response; `errors` is included
    only on validation errors (HTTP 400) and maps field name → list of slugs.
    """

    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    detail: str = Field(
        ..., description="Human-readable explanation specific to this occurrence."
    )
    errors: dict[str, list[str]] | None = Field(
        None,
        description="Field-level validation errors (present only on 400 responses).",
    )
    instance: str | None = Field(
        None,
        description="URI reference identifying the specific occurrence of the problem.",
    )
    request_id: str | None = Field(
        None, description="Correlation ID for request tracing."
    )
    status: int = Field(..., description="HTTP status code.", ge=0)
    title: str = Field(..., description="Short human-readable summary of the error.")
    type: str = Field(..., description="A URI reference identifying the error type.")


class FeatureSource(Enum):
    default = "default"
    override = "override"


class FeatureValueType(Enum):
    bool = "bool"
    string = "string"
    int = "int"
    enum_set = "enum_set"
    json = "json"


class ListApiKeysResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[ApiKeyResponse]


class LoginRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    password: str
    username_or_email: str


class MemberBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    email: str
    role_codes: list[str]
    user_id: UUID
    username: str


class MembershipDto(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    joined_at: AwareDatetime
    org_id: UUID
    org_name: str
    role_codes: list[str]


class OrganisationBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    business: str
    id: UUID
    name: str
    role_codes: list[str]


class PagedMembers(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[MemberBody]
    next_cursor: str | None = None


class PagedOrganisations(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[OrganisationBody]
    next_cursor: str | None = None


class PasswordResetConfirmBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    new_password: str
    token: str


class PasswordResetRequestBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    email: str


class PutFeatureRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    value: Any


class RegisterRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    email: str
    org_id: UUID
    password: str
    role_code: str
    username: str


class RegisterResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    user_id: UUID


class RemoveUserFromOrganisationRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    org_id: UUID
    user_id: UUID


class RotateApiKeyRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    name: str | None = None
    scopes: list[str] | None = Field(
        None,
        description='`null` = keep existing scopes; `Some(Some(scopes))` = override; `Some(None)` is\nnot representable in JSON, so the wire-level meaning of an absent field is\n"inherit existing".',
    )


class ServiceAccountResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    created_at: AwareDatetime
    description: str | None = None
    last_used_at: AwareDatetime | None = None
    name: str
    organisation_id: UUID
    user_id: UUID


class SwitchOrgRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    org_id: UUID


class TokenResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    token: str


class UpdateChannelRequest(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    channel_type: ChannelType
    description: str | None = None
    is_active: bool
    name: str


class UserSummaryDto(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    created_at: AwareDatetime
    email: str
    id: UUID
    memberships: list[MembershipDto]
    username: str


class ChannelBody(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    api_key: str
    channel_type: ChannelType
    created_at: AwareDatetime
    description: str | None = None
    id: UUID
    is_active: bool
    name: str
    organisation_id: UUID
    updated_at: AwareDatetime


class EvaluatedFeature(BaseModel):
    """
    Effective value for an (org, slug) pair, with provenance for UI/audit.
    """

    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    self_service: bool
    slug: str
    source: FeatureSource
    value: Any
    value_type: FeatureValueType


class FeatureDefinition(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    default_value: Any
    description: str
    self_service: bool
    slug: str
    value_type: FeatureValueType


class ListServiceAccountsResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[ServiceAccountResponse]
    next_cursor: str | None = None


class ListUsersResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[UserSummaryDto]
    next_cursor: str | None = None


class LoginResponse(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    active_org_id: UUID
    memberships: list[MembershipDto]
    token: str
    user_id: UUID


class PagedChannels(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="allow",
    )
    items: list[ChannelBody]
    next_cursor: str | None = None
