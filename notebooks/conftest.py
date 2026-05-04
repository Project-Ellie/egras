"""Pytest fixture that skips notebook tests if the egras server isn't up."""
import os
import pytest
import requests

DEFAULT_BASE_URL = os.environ.get("EGRAS_BASE_URL", "http://localhost:8080")


@pytest.fixture(autouse=True, scope="session")
def server_up():
    try:
        r = requests.get(DEFAULT_BASE_URL.rstrip("/") + "/health", timeout=2)
        r.raise_for_status()
    except Exception as e:
        pytest.skip(f"egras server not reachable at {DEFAULT_BASE_URL}: {e}")
