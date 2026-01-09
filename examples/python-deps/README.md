# Python External Dependencies Example

This example demonstrates using external Python packages (from PyPI)
with Buck2 through the turnkey framework.

## Current Status

**Not yet functional** - requires third-party Python package setup.

## Required Setup

Buck2 requires explicit handling of Python third-party dependencies.
Unlike pip, Buck2 doesn't automatically fetch packages.

### Options:

1. **Vendor packages**: Download wheels and create python_library targets
2. **Use pip_install rules**: Some Buck2 setups use custom pip rules
3. **Nix integration**: Use Nix to provide Python packages in a third-party cell

### Example third-party setup:

```python
# third-party/python/BUCK
load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "click",
    srcs = glob(["click/**/*.py"]),
    visibility = ["PUBLIC"],
)
```

## Future: Turnkey Integration

A future turnkey feature could automate this with:
- A `python-deps.toml` similar to `go-deps.toml`
- A `pydeps-gen` tool similar to `godeps-gen`
- Nix-based Python package resolution
