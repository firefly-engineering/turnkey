# Prelude Extensions

Add custom Buck2 rules to the prelude.

## Overview

Prelude extensions live in `nix/buck2/prelude-extensions/`. They're copied into the prelude during the Nix build.

## Directory Structure

```
nix/buck2/prelude-extensions/
└── mylang/
    ├── providers.bzl     # Provider definitions
    ├── toolchain.bzl     # Toolchain rule
    ├── mylang_library.bzl
    ├── mylang_binary.bzl
    └── mylang.bzl        # Convenience exports
```

## Creating an Extension

### 1. Create Provider

`providers.bzl`:

```python
MylangToolchainInfo = provider(
    doc = "Mylang toolchain information.",
    fields = {
        "compiler": provider_field(typing.Any, default = None),
    },
)
```

### 2. Create Toolchain Rule

`toolchain.bzl`:

```python
load(":providers.bzl", "MylangToolchainInfo")

def _system_mylang_toolchain_impl(ctx):
    compiler_path = ctx.attrs.compiler_path
    return [
        DefaultInfo(),
        MylangToolchainInfo(
            compiler = RunInfo(args = cmd_args(compiler_path)),
        ),
    ]

system_mylang_toolchain = rule(
    impl = _system_mylang_toolchain_impl,
    attrs = {
        "compiler_path": attrs.string(),
    },
    is_toolchain_rule = True,
)
```

### 3. Create Build Rules

`mylang_binary.bzl`:

```python
load(":providers.bzl", "MylangToolchainInfo")

def _mylang_binary_impl(ctx):
    toolchain = ctx.attrs._toolchain[MylangToolchainInfo]
    out = ctx.actions.declare_output(ctx.label.name)

    ctx.actions.run(
        cmd_args(toolchain.compiler.args, ctx.attrs.srcs, "-o", out.as_output()),
        category = "mylang_compile",
    )

    return [DefaultInfo(default_output = out)]

mylang_binary = rule(
    impl = _mylang_binary_impl,
    attrs = {
        "srcs": attrs.list(attrs.source()),
        "_toolchain": attrs.toolchain_dep(
            default = "toolchains//:mylang",
            providers = [MylangToolchainInfo],
        ),
    },
)
```

### 4. Export Rules

`mylang.bzl`:

```python
load(":mylang_binary.bzl", _mylang_binary = "mylang_binary")
load(":toolchain.bzl", _system_mylang_toolchain = "system_mylang_toolchain")

mylang_binary = _mylang_binary
system_mylang_toolchain = _system_mylang_toolchain
```

## Building

Extensions are included when you rebuild the prelude:

```bash
git add nix/buck2/prelude-extensions/mylang/
nix build .#turnkey-prelude
```

## Examples

See existing extensions:
- `typescript/` - TypeScript compiler integration
- `mdbook/` - Documentation builder
