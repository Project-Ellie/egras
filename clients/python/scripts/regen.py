#!/usr/bin/env python3
"""Regenerate ``egras_client/models.py`` from ``docs/openapi.json``.

This wraps `datamodel-code-generator
<https://github.com/koxudaxi/datamodel-code-generator>`_ with two
project-specific concerns:

1. Some response refs in the spec point at ``crate.errors.ErrorBody`` (a
   utoipa quirk). The generator can't emit a class with a ``.`` in its
   name — we rewrite the spec in memory, mapping the dotted name to plain
   ``ErrorBody`` (the schema is identical).

2. We always pin the generator's output style: Pydantic v2, ``from __future__
   import annotations``, snake_case fields, and ``Optional[T]`` for
   ``nullable: true``.

The output is written atomically over ``egras_client/models.py``. To avoid a
spurious diff on every run, the generator's banner is replaced with a
stable comment.

Usage
-----
::

    pip install -e clients/python[dev]
    python clients/python/scripts/regen.py

The optional ``--check`` flag exits non-zero if the regenerated file would
differ from what's on disk — this is what the CI / pre-push hook calls.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SPEC_PATH = REPO_ROOT / "docs" / "openapi.json"
OUTPUT_PATH = REPO_ROOT / "clients" / "python" / "egras_client" / "models.py"

# Schema names that must be rewritten before we hand the spec off to the
# generator. Maps the spec name to a Python-valid name.
NAME_REWRITES = {
    "crate.errors.ErrorBody": "ErrorBody",
}


HEADER = '''"""Pydantic v2 models mirroring the egras OpenAPI schema.

AUTO-GENERATED — do not edit by hand. The source of truth is
``docs/openapi.json``; regenerate via:

    python clients/python/scripts/regen.py
"""
'''


def normalise_spec(spec: dict) -> dict:
    """Apply spec rewrites needed for clean codegen.

    Two transforms:

    1. :data:`NAME_REWRITES` renames schemas with non-identifier characters
       (e.g. ``crate.errors.ErrorBody`` → ``ErrorBody``) and updates every
       ``$ref`` to match.

    2. ``type: object`` + ``nullable: true`` properties are converted to
       ``anyOf: [{type: object}, {type: null}]``. datamodel-code-generator
       (as of 0.56) honours ``nullable: true`` for scalar types but drops
       it for free-form objects, so without this transform a required
       nullable object would be emitted as a non-Optional ``dict``.
       Returns a deep-copied, mutated spec.
    """
    out = json.loads(json.dumps(spec))  # deep copy via JSON round-trip

    schemas = out.get("components", {}).get("schemas", {})
    for src, dst in NAME_REWRITES.items():
        if src in schemas:
            if dst in schemas:
                schemas.pop(src)
            else:
                schemas[dst] = schemas.pop(src)

    def walk(node):
        if isinstance(node, dict):
            # Free-form `type: object` (no fixed `properties`, no
            # `additionalProperties`) means "any JSON value" in egras's
            # schemas (e.g. EvaluatedFeature.value can be a list, a bool,
            # an int, or a dict). Without this rewrite the generator emits
            # `dict[str, Any]`, which would reject the list returned by
            # the auth.api_key_headers feature.
            #
            # We replace such schemas with the empty schema (= any), and
            # if it was nullable, we widen with anyOf-null.
            if (
                node.get("type") == "object"
                and "properties" not in node
                and "additionalProperties" not in node
            ):
                nullable = node.pop("nullable", False) is True
                if nullable:
                    node.clear()
                    node["anyOf"] = [{}, {"type": "null"}]
                else:
                    # Empty schema => Any. Strip everything except
                    # description/title (preserved for nicer codegen).
                    keep = {
                        k: node[k] for k in ("description", "title") if k in node
                    }
                    node.clear()
                    node.update(keep)
                return  # nothing inside to recurse into

            for k, v in list(node.items()):
                if k == "$ref" and isinstance(v, str):
                    for src, dst in NAME_REWRITES.items():
                        v = v.replace(f"/{src}", f"/{dst}")
                    node[k] = v
                else:
                    walk(v)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(out)
    return out


def run_codegen(normalised_spec: dict) -> str:
    """Invoke datamodel-code-generator and return the generated source.

    We invoke via ``sys.executable -m datamodel_code_generator`` rather
    than relying on the ``datamodel-codegen`` console script being on
    ``$PATH``. This makes ``regen.py`` work whenever the package was
    installed into the venv that is *currently running* the script — the
    common case (pip install -e clients/python[dev]) — even when that
    venv's ``bin/`` directory has not been activated in the calling shell.
    """
    try:
        import datamodel_code_generator  # noqa: F401
    except ImportError:
        sys.exit(
            "datamodel-code-generator is not installed in this interpreter. "
            "Install with: pip install -e 'clients/python[dev]'"
        )

    with tempfile.TemporaryDirectory() as tmp:
        spec_file = Path(tmp) / "spec.json"
        out_file = Path(tmp) / "models.py"
        spec_file.write_text(json.dumps(normalised_spec))

        cmd = [
            sys.executable, "-m", "datamodel_code_generator",
            "--input", str(spec_file),
            "--input-file-type", "openapi",
            "--output", str(out_file),
            "--output-model-type", "pydantic_v2.BaseModel",
            "--use-standard-collections",      # list[X] not List[X]
            "--use-union-operator",            # X | None not Optional[X] (3.10+)
            "--use-schema-description",
            "--use-double-quotes",
            "--target-python-version", "3.10",
            "--snake-case-field",
            "--field-constraints",
            "--enable-faux-immutability",      # frozen models — safer in notebooks
        ]
        subprocess.run(cmd, check=True)
        return out_file.read_text()


def post_process(generated: str) -> str:
    """Stabilise the generator's output.

    Three changes are applied to the raw generator output:

    1. Strip the codegen banner (it has a timestamp + version, which would
       cause a meaningless diff on every regen) and replace with our own.
    2. Inject ``extra="allow"`` into every ``ConfigDict``. This makes the
       client forward-compatible: when the server adds a new optional
       field, existing clients keep working instead of raising
       ``ValidationError``.
    3. (No-op today) Reserved hook for any future stable-output rewrites.
    """
    lines = generated.splitlines(keepends=True)
    i = 0
    if lines and lines[0].startswith("# generated by datamodel-codegen"):
        while i < len(lines) and lines[i].strip() != "":
            i += 1
        if i < len(lines) and lines[i].strip() == "":
            i += 1
    body = "".join(lines[i:])

    # Inject extra="allow" into ConfigDict. The generator emits one of:
    #     model_config = ConfigDict(\n        frozen=True,\n    )
    # We turn that into:
    #     model_config = ConfigDict(\n        frozen=True, extra="allow",\n    )
    body = body.replace(
        "model_config = ConfigDict(\n        frozen=True,\n    )",
        'model_config = ConfigDict(\n        frozen=True, extra="allow",\n    )',
    )

    return HEADER + "\n" + body


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero if the regenerated file differs from disk (no write)",
    )
    args = p.parse_args()

    if not SPEC_PATH.exists():
        sys.exit(f"spec not found: {SPEC_PATH}")

    spec = json.loads(SPEC_PATH.read_text())
    normalised = normalise_spec(spec)
    generated = post_process(run_codegen(normalised))

    if args.check:
        existing = OUTPUT_PATH.read_text() if OUTPUT_PATH.exists() else ""
        if existing != generated:
            sys.stderr.write(
                "models.py is stale. Run: python clients/python/scripts/regen.py\n"
            )
            return 1
        return 0

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(generated)
    print(f"wrote {OUTPUT_PATH.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
