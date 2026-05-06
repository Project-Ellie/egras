"""Wrappers for ``/api/v1/features/*`` — feature flags."""

from __future__ import annotations

from typing import Union
from uuid import UUID

from ..models import EvaluatedFeature, FeatureDefinition, PutFeatureRequest
from ._base import _Api


class FeaturesApi(_Api):
    """Feature flag definitions and per-org overrides.

    The server distinguishes:

    * **definitions** — global, code-defined catalogue (immutable at runtime).
    * **evaluated features** — what an org actually sees, with ``source``
      indicating whether the value comes from the global default or an
      org-specific override.
    """

    def list_definitions(self) -> list[FeatureDefinition]:
        """``get_definitions`` — GET /api/v1/features. Returns the global catalogue."""
        raw = self._c.request("GET", "/api/v1/features")
        return [FeatureDefinition.model_validate(item) for item in raw]

    def list_org_features(self, org_id: Union[UUID, str]) -> list[EvaluatedFeature]:
        """``get_org_features`` — GET /api/v1/features/orgs/{org_id}.

        Returns the resolved values for ``org_id`` — every feature in the
        global catalogue, with the override (if any) merged in.
        """
        raw = self._c.request("GET", f"/api/v1/features/orgs/{org_id}")
        return [EvaluatedFeature.model_validate(item) for item in raw]

    def put_org_feature(
        self,
        org_id: Union[UUID, str],
        slug: str,
        body: Union[PutFeatureRequest, dict],
    ) -> EvaluatedFeature:
        """``put_org_feature`` — PUT /api/v1/features/orgs/{org_id}/{slug}.

        Sets an override. The supplied ``value`` must match the feature's
        declared ``value_type`` (server validates).
        """
        return self._c.request(
            "PUT", f"/api/v1/features/orgs/{org_id}/{slug}",
            json=body, response_model=EvaluatedFeature,
        )

    def delete_org_feature(self, org_id: Union[UUID, str], slug: str) -> None:
        """``delete_org_feature`` — DELETE /api/v1/features/orgs/{org_id}/{slug}.

        Removes the override; the org reverts to the global default. 204.
        """
        self._c.request(
            "DELETE", f"/api/v1/features/orgs/{org_id}/{slug}",
            expect_no_content=True,
        )
