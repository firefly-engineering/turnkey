# Python Workspaces

Turnkey lays out Python source as a [uv workspace][uv-workspaces], in parallel to the Cargo workspace pattern used for Rust. A single `uv.lock` resolves every Python package in the monorepo against a consistent dependency set, while each package keeps its own `pyproject.toml` declaring exactly what it consumes.

Two tracks run side by side over the same source:

- **uv track** — `uv sync`, `uv run`, IDE language servers, REPL. Members are installed editable so source edits are reflected immediately.
- **Buck2 track** — `tk build`, `tk test`. External packages are vendored into the `pydeps` cell built from `python-deps.toml`.

[uv-workspaces]: https://docs.astral.sh/uv/concepts/projects/workspaces/

## Repository Layout

```
/repo/
├── pyproject.toml                       # Workspace root: members + uv.lock anchor
├── uv.lock                              # Single resolved lockfile (managed by uv)
├── pylock.toml                          # PEP 751 export from uv.lock
├── python-deps.toml                     # Generated for Buck2/Nix from pylock.toml
└── src/python/<member>/
    ├── pyproject.toml                   # [project] + hatchling build backend
    ├── rules.star                       # Buck2 targets for the member
    └── turnkey/<member>/                # Source under shared turnkey.* namespace
        ├── __init__.py
        └── ...
```

Tests live in a sibling `tests/` directory inside each member, kept outside the importable namespace.

## The `turnkey.*` Namespace Convention

Every workspace member contributes a subpackage under the shared `turnkey` [PEP 420 implicit namespace package][pep-420]. No member defines a top-level `turnkey/__init__.py`; Python's import system resolves `turnkey.cargo`, `turnkey.cfg`, etc. by walking every `sys.path` entry that exposes a `turnkey/<name>/` directory.

[pep-420]: https://peps.python.org/pep-0420/

### Downstream Projects: Pick Your Own Namespace

The `turnkey.*` prefix is **this repository's** namespace. If you adopt the same workspace pattern in a different monorepo, choose a namespace specific to your organisation — e.g. `acme.<name>` — to avoid colliding with packages on PyPI or other turnkey-based repos. The mechanics are identical; substitute `turnkey` for your namespace throughout this guide.

## Member `pyproject.toml`

Each library member uses the [hatchling][hatchling] backend and points it at the `turnkey/` directory:

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "turnkey-cargo"
version = "0.1.0"
description = "Cargo manifest and feature-graph utilities"
requires-python = ">=3.11"
dependencies = [
    "turnkey-cfg",       # cross-member dep
]

[tool.uv.sources]
turnkey-cfg = { workspace = true }

[tool.hatch.build.targets.wheel]
packages = ["turnkey"]   # everything under turnkey/<name>/ is the wheel content
```

`packages = ["turnkey"]` is the key line: it tells hatchling that the wheel's content is whatever lives under the `turnkey/` directory of this member. Combined with PEP 420 namespace resolution, every member ships only its own `turnkey/<name>/` slice without anyone owning `turnkey/__init__.py`.

[hatchling]: https://hatch.pypa.io/

### Cross-Member Dependencies

Declare the dep under `[project] dependencies` with the bare package name, then pin its source to the workspace under `[tool.uv.sources]`:

```toml
dependencies = ["turnkey-cfg"]

[tool.uv.sources]
turnkey-cfg = { workspace = true }
```

This mirrors `Cargo.toml`'s `serde.workspace = true` pattern — the consumer member doesn't pin a version, the lockfile reconciles it.

### External Dependencies

Declare externals in the member that consumes them, never the workspace root:

```toml
# src/examples/python-hello-deps/pyproject.toml
[project]
name = "turnkey-example-python-hello-deps"
dependencies = ["six>=1.16.0"]
```

The single `uv.lock` at the workspace root resolves every external version-consistently across members.

### Non-Packaged Members

Some members exist only to declare dependencies, not to be installed (typical for application-like entrypoints or examples). Mark them non-packaged:

```toml
[project]
name = "turnkey-example-python-hello-deps"
version = "0.1.0"
dependencies = ["six>=1.16.0"]

[tool.uv]
package = false        # uv won't build/install this member
```

No `[build-system]` is required. uv still resolves the member's dependencies as part of the workspace lock.

## Root `pyproject.toml`

The workspace root anchors membership and the shared lockfile:

```toml
[project]
name = "turnkey"
version = "0.1.0"
requires-python = ">=3.11"

# Listing members as dependencies makes the default `uv sync` install all
# of them in one shot — no `--all-packages` flag needed.
dependencies = [
    "turnkey-buck",
    "turnkey-buildsystem",
    "turnkey-cargo",
    "turnkey-cfg",
    "turnkey-example-python-hello",
    "turnkey-example-python-hello-deps",
]

[dependency-groups]
# Dev tooling — auto-installed by 'uv sync' so 'uv run pytest' Just Works.
dev = ["pytest>=7.0"]

[tool.uv.workspace]
members = [
    "src/python/cargo",
    "src/python/buck",
    "src/python/buildsystem",
    "src/python/cfg",
    "src/examples/python-hello",
    "src/examples/python-hello-deps",
]

[tool.uv.sources]
turnkey-buck = { workspace = true }
turnkey-buildsystem = { workspace = true }
turnkey-cargo = { workspace = true }
turnkey-cfg = { workspace = true }
turnkey-example-python-hello = { workspace = true }
turnkey-example-python-hello-deps = { workspace = true }

[tool.uv]
package = false        # the root itself isn't a packaged project
```

## Buck2 Integration

Member source paths are spelled relative to the member's `rules.star`:

```python
load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cargo",
    srcs = [
        "turnkey/cargo/__init__.py",
        "turnkey/cargo/features.py",
        "turnkey/cargo/toml.py",
    ],
    base_module = "",
    deps = ["//src/python/cfg:cfg"],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test_toml",
    srcs = ["tests/test_toml.py"],
    base_module = "tests",
    deps = [":cargo"],
)
```

`base_module = ""` tells Buck2 to install sources at their declared `srcs` paths, so files land at `turnkey/cargo/...` in the runtime tree — matching the import prefix the rest of the codebase uses.

## Adding or Updating Dependencies

```bash
# 1. Edit the member that needs the dep
$EDITOR src/python/cargo/pyproject.toml      # add to [project] dependencies

# 2. Regenerate the lock
uv lock

# 3. Refresh editable installs (optional but recommended)
uv sync

# 4. Export to PEP 751 lock for the Buck2 pipeline
#    --all-packages: include externals from every member
#    --no-dev:       exclude dev tooling (pytest etc.) from the pydeps cell
uv export --all-packages --no-dev --format pylock.toml -o pylock.toml

# 5. Refresh python-deps.toml for the pydeps cell
#    (tk sync picks this up automatically when pylock.toml is newer)
tk sync
```

Steps 2–4 are manual today; future work can fold them into `tk sync` as a pre-step.

## Running Code

| Task                            | uv track                                | Buck2 track                                       |
| ------------------------------- | --------------------------------------- | ------------------------------------------------- |
| Run all tests                   | `uv run pytest`                         | `tk test //src/python/...`                        |
| Run a single member's tests     | `uv run pytest src/python/cargo`        | `tk test //src/python/cargo:test_toml`            |
| Run an example                  | `uv run --package <pkg-name> <script>`  | `tk run //src/examples/python-hello-deps:python-hello-deps` |
| REPL with members available     | `uv run python`                         | n/a                                               |
| IDE language server             | Point at `.venv/bin/python`             | n/a                                               |

Both tracks resolve external dependencies the same way (uv.lock is the single source of truth), but the install paths differ: the uv track installs into `.venv/`, the Buck2 track materialises external packages into `.turnkey/pydeps/vendor/<name>/`.

## See Also

- [Managing Dependencies](./dependencies.md) — overall dependency flow across all languages.
- [Python](../languages/python.md) — Buck2 build rules for Python targets.
