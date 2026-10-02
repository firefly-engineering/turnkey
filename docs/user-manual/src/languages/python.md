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
dependency graph). Schema 3 records each distribution's pure wheel, with
markers and extras:

```toml
schema_version = 3

[deps.requests]
version = "2.32.3"
# The Nix hash of the unpacked wheel
hash = "sha256-..."
# The locked py3-none-any wheel
url = "https://files.pythonhosted.org/.../requests-2.32.3-py3-none-any.whl"
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

A file from before schema 3 records sdists, and fails evaluation asking for
`tk sync`, which regenerates it.

### Distributions are their locked wheels

The pydeps cell vendors each distribution as the pure (`py3-none-any`)
wheel `uv` locked for it, unpacked: the layout an installer puts in
site-packages
([ADR 0013](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0013-pydeps-distributions-are-their-locked-wheels.md)).
So `import requests` works although requests keeps its code under `src/` in
its sdist, and an sdist's `setup.py`, `tests/` and docs are not in the
target.

- The wheel's `<name>.data/purelib` and `platlib` are merged into its root,
  as an installer would; the rest of `<name>.data/` (scripts, headers, data)
  is dropped. Fixups, then user patches, apply to that tree.
- The distribution's `python_library` holds everything the wheel installs:
  its `.py` files as `srcs`, and every other file as `resources`. That
  includes data files (such as `certifi`'s `cacert.pem`) and the
  `*.dist-info` directory, so `importlib.metadata.version("requests")` and
  entry points work.
- **A distribution with no pure wheel fails `pydeps-gen`**, which names it:
  one with only platform-specific (compiled) wheels, and one with only an
  sdist. turnkey doesn't build sdists into wheels, and doesn't vendor
  platform wheels yet
  ([#248](https://github.com/firefly-engineering/turnkey/issues/248)).

`src/examples/python-requests` uses `requests` and `certifi` this way.

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
