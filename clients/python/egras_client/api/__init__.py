"""Endpoint wrappers, grouped by URL prefix.

Each module exposes one ``*Api`` class instantiated by :class:`Client`. Methods
are 1:1 with OpenAPI operations; the operation id is mentioned in each
docstring so a grep against ``docs/openapi.json`` is always cheap.
"""
