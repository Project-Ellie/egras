# egras-client

Python client for the [egras](https://github.com/Project-Ellie/egras) REST API.
Sync, typed (Pydantic v2), driven by `docs/openapi.json` so it stays in sync
with the server.

## Install (editable, for the egras monorepo)

From the repo root:

```bash
pip install -e 'clients/python[dev]'
```

The single quotes matter under zsh — unquoted, `[dev]` is read as a glob
character class and the install fails with `zsh: no matches found`.

If you only need the runtime client (no test/dev extras), drop `[dev]`:

```bash
pip install -e clients/python
```

The notebooks under `notebooks/scenarios/` don't need this step — they install
the client editable via `notebooks/requirements.txt` (`pip install -r
requirements.txt` from `notebooks/`).

## Usage

```python
from egras_client import Client
from egras_client.helpers import login_operator, operator_credentials

email, pw = operator_credentials()                # env-driven, with defaults
token = login_operator("http://localhost:8080", email, pw)
c = Client("http://localhost:8080", jwt=token)

# Each domain is a property on Client; methods are typed and grouped by tag.
me_orgs = c.tenants.list_my_organisations()       # -> PagedOrganisations
echo    = c.echo.echo_post({"hi": "there"})       # -> EchoResponse
users   = c.users.list_users()                    # -> ListUsersResponse
```

API-key auth (for service accounts):

```python
c = Client("http://localhost:8080", api_key=plaintext)
c.echo.echo_get()
```

Errors map to typed exceptions; the RFC 7807 problem body is preserved on the
exception:

```python
from egras_client.errors import ApiError, Unauthorized, Forbidden

try:
    c.echo.echo_get()
except Unauthorized as e:
    print(e.problem.title, e.problem.detail)
```

## Regenerating after a server change

The Pydantic models in `egras_client/models.py` are auto-generated from the
committed OpenAPI spec. After changing handlers/schemas in the Rust code:

```bash
# 1. Regenerate the OpenAPI spec from the server
cargo run -- dump-openapi > docs/openapi.json

# 2. Regenerate Pydantic models
python clients/python/scripts/regen.py

# 3. Verify no path is missing a wrapper
python clients/python/scripts/check_drift.py
```

The pre-push hook runs steps 1+3 automatically; if drift is detected the push
is blocked until you also touch the matching api/ wrapper.

## Layout

```
egras_client/
├── client.py     # Client class: HTTP, auth, JSON, error mapping
├── errors.py     # ApiError hierarchy, ProblemBody (RFC 7807)
├── models.py     # AUTO-GENERATED Pydantic models — do not edit
├── api/          # Endpoint wrappers, one file per OpenAPI tag
└── helpers.py    # High-level scenario shortcuts (login, bootstrap)
```
