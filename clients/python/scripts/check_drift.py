#!/usr/bin/env python3
"""Fail if any path in ``docs/openapi.json`` lacks a wrapper in ``egras_client``.

The drift check is structural, not semantic: for every ``operationId`` in
the spec, we grep the generated/hand-written ``egras_client/api/*.py``
modules for that operation id appearing inside a docstring. The
``security.py``/``tenants.py``/etc. modules already include each operation
id in the method docstring on purpose, so this check costs nothing extra
to maintain.

Exits 0 if everything in the spec is wrapped, 1 otherwise.

Usage
-----
::

    python clients/python/scripts/check_drift.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SPEC_PATH = REPO_ROOT / "docs" / "openapi.json"
API_DIR = REPO_ROOT / "clients" / "python" / "egras_client" / "api"


def collect_operation_ids(spec: dict) -> list[str]:
    ops: list[str] = []
    for path, methods in spec.get("paths", {}).items():
        for method, op in methods.items():
            if method not in {"get", "post", "put", "delete", "patch"}:
                continue
            op_id = op.get("operationId")
            if not op_id:
                sys.exit(f"spec error: missing operationId at {method.upper()} {path}")
            ops.append(op_id)
    return ops


def collect_documented_op_ids() -> str:
    """Concatenate every api/*.py file's source for substring search."""
    chunks: list[str] = []
    for f in sorted(API_DIR.glob("*.py")):
        chunks.append(f.read_text())
    return "\n".join(chunks)


def main() -> int:
    if not SPEC_PATH.exists():
        sys.exit(f"spec not found: {SPEC_PATH}")
    spec = json.loads(SPEC_PATH.read_text())

    operation_ids = collect_operation_ids(spec)
    api_source = collect_documented_op_ids()

    missing = [op for op in operation_ids if op not in api_source]
    if missing:
        sys.stderr.write(
            "Drift detected: the following operationIds from the OpenAPI spec\n"
            "have no matching wrapper under egras_client/api/. Add a method\n"
            "whose docstring mentions the operationId:\n\n"
        )
        for op in missing:
            sys.stderr.write(f"  - {op}\n")
        return 1

    print(f"OK — all {len(operation_ids)} operationIds are wrapped.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
