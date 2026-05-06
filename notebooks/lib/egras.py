"""Compatibility shim for older notebooks.

The notebook helpers used to live here. They now live in the
``egras-client`` package under ``clients/python/`` — see
``notebooks/scenarios/01_echo_smoke.ipynb`` for the modern usage.

This file remains only so an old in-flight notebook checkout doesn't
crash on import; new notebooks should import from ``egras_client``
directly.
"""

import warnings

from egras_client.helpers import (  # noqa: F401  (re-exported for back-compat)
    login_operator,
    operator_credentials,
)

warnings.warn(
    "notebooks.lib.egras is deprecated; import from `egras_client` instead "
    "(e.g. `from egras_client import Client; "
    "from egras_client.helpers import operator_credentials, login_operator`).",
    DeprecationWarning,
    stacklevel=2,
)
