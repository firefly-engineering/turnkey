# Python Support

Turnkey provides Python support with Buck2 integration.

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

## Auto-Sync

The `uv` command is wrapped to auto-sync:

```bash
uv add requests  # Triggers sync
```
