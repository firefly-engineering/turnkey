# Python Support

Turnkey provides Python support with Buck2 integration.

> Python source in this repo is laid out as a [uv workspace](../workflows/python-workspace.md), with each package owning its own `pyproject.toml` and contributing to a shared `turnkey.*` PEP 420 namespace. This page covers the Buck2 build rules; read the workspace workflow guide first for the overall layout and the uv/Buck2 dual-track model.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
python = {}
uv = {}
pydeps-gen = {}
```

Enable Python dependencies in `flake.nix`:

```nix
turnkey.toolchains.buck2.python = {
  enable = true;
  depsFile = ./python-deps.toml;
};
```

## Project Structure

```
my-project/
├── pyproject.toml
├── uv.lock
├── python-deps.toml      # Generated from uv.lock
└── python/
    └── mypackage/
        ├── __init__.py
        ├── main.py
        └── rules.star
```

## Build Rules

In `rules.star`:

```python
load("@prelude//python:python.bzl", "python_library", "python_binary", "python_test")

python_library(
    name = "mypackage",
    srcs = glob(["**/*.py"]),
    deps = ["pydeps//requests:requests"],
)

python_binary(
    name = "main",
    main = "main.py",
    deps = [":mypackage"],
)

python_test(
    name = "test",
    srcs = ["test_main.py"],
    deps = [":mypackage"],
)
```

## External Dependencies

Reference packages via the `pydeps` cell:

```python
deps = [
    "pydeps//requests:requests",
    "pydeps//click:click",
]
```

### `python-deps.toml`

`pydeps-gen` writes it from `pylock.toml` (what to fetch) and `uv.lock` (the
dependency graph). Schema 2 records markers and extras:

```toml
schema_version = 2

[deps.requests]
version = "2.32.3"
hash = "sha256-..."
url = "https://files.pythonhosted.org/..."
# The lock's marker for installing the package at all, when it has one
marker = "python_version >= '3.8'"
# Its dependencies: each package's key, with its marker and the extras it
# asks for, when it has them
dependencies = [
    { name = "urllib3" },
    { name = "colorama", marker = "sys_platform == 'win32'" },
]
# The extras some package or workspace member asks it for
requested_extras = ["socks"]

# Its extras, and the dependencies each adds
[deps.requests.extras]
"socks" = [{ name = "pysocks" }]
```

The pydeps cell uses them: a package's target depends on its dependencies
and on those of its requested extras, each where its marker holds. Markers
are evaluated on every platform in `buck2.platforms`, for the Python
toolchain's version: a dependency some platforms get is a `select()` on the
platform, and one none gets is left out.

## Markers and Extras

Rules sync reads a workspace member's `pyproject.toml` as well as its
imports:

- A dependency in `[project] dependencies` with a **platform marker**
  (`sys_platform`, `platform_system`, `platform_machine`, `os_name`) is
  written as a `select()` over the platforms, like any
  platform-conditional dep.
- **Other markers** (`python_version`, `implementation_name`, ...) are
  fixed by the Python toolchain: they are evaluated once, for the `python3`
  of the shell, and a dependency whose marker doesn't hold is left out.
- **Extras** are a variant: a target declares the extras it's built with on
  turnkey's prelude attribute `extras` (possibly a `select()`), and sync
  adds the dependencies they enable, whether its sources import them or
  not. A dependency declared only as an extra's is kept only on a target
  built with that extra.

```python
python_library(
    name = "app-full",
    extras = ["socks"],
    deps = [...],  # sync adds the socks extra's dependencies
)
```

## Auto-Sync

The `uv` command is wrapped to auto-sync:

```bash
uv add requests  # Triggers sync
```
