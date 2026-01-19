# Prelude Extensions

This document covers how to add custom Buck2 rules to the prelude and the various customization approaches available.

## Overview

The Buck2 prelude is a collection of Starlark rules that provide build functionality for various languages (Go, Rust, Python, C++, etc.). It's the "standard library" of build rules that ships with Buck2.

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

MylangLibraryInfo = provider(
    doc = "Information about a mylang library.",
    fields = {
        "output": provider_field(typing.Any, default = None),
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
        "compiler_path": attrs.string(
            doc = "Path to the mylang compiler binary",
        ),
    },
    is_toolchain_rule = True,
    doc = "System-provided mylang toolchain.",
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
        cmd_args(
            toolchain.compiler.args,
            ctx.attrs.srcs,
            "-o",
            out.as_output(),
        ),
        category = "mylang_compile",
        identifier = ctx.label.name,
    )

    return [
        DefaultInfo(default_output = out),
        RunInfo(args = cmd_args(out)),
    ]

mylang_binary = rule(
    impl = _mylang_binary_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            doc = "Source files to compile",
        ),
        "_toolchain": attrs.toolchain_dep(
            default = "toolchains//:mylang",
            providers = [MylangToolchainInfo],
        ),
    },
    doc = "Build a mylang executable.",
)
```

### 4. Export Rules

`mylang.bzl`:

```python
load(":mylang_binary.bzl", _mylang_binary = "mylang_binary")
load(":mylang_library.bzl", _mylang_library = "mylang_library")
load(":toolchain.bzl", _system_mylang_toolchain = "system_mylang_toolchain")
load(":providers.bzl", _MylangToolchainInfo = "MylangToolchainInfo")

mylang_binary = _mylang_binary
mylang_library = _mylang_library
system_mylang_toolchain = _system_mylang_toolchain
MylangToolchainInfo = _MylangToolchainInfo
```

### 5. Add Toolchain Mapping

Edit `nix/buck2/mappings.nix`:

```nix
mylang = {
  skip = false;
  targets = [{
    name = "mylang";
    rule = "system_mylang_toolchain";
    load = "@prelude//mylang:toolchain.bzl";
    visibility = [ "PUBLIC" ];
    dynamicAttrs = registry: {
      compiler_path = "${registry.mylang}/bin/mylang";
    };
  }];
  implicitDependencies = [ ];
  runtimeDependencies = [ ];
};
```

## Building

Extensions are included when you rebuild the prelude:

```bash
git add nix/buck2/prelude-extensions/mylang/
nix build .#turnkey-prelude
```

## Customization Approaches

### Approach 1: Extension Cell Pattern

Create a separate cell for custom rules alongside the standard prelude:

```
project/
├── prelude/           # Standard prelude (submodule or external)
├── prelude-custom/    # Custom extensions
│   ├── BUCK
│   ├── platforms/
│   ├── toolchains/
│   └── rules/
└── .buckconfig
```

```ini
[cells]
prelude = prelude
prelude-custom = prelude-custom

[external_cells]
prelude = bundled

[build]
execution_platforms = prelude-custom//platforms:default
```

**Pros:**
- Clean separation of concerns
- Can still use bundled prelude for core rules
- Easy to track what's custom vs standard
- No fork maintenance burden

**Cons:**
- Two cells to manage
- Must understand which rules come from where

### Approach 2: Custom Rules Outside Prelude

Define rules anywhere in your project - they don't need to be in the prelude:

```python
# rules/my_rules.bzl
def my_custom_rule_impl(ctx):
    # Implementation
    pass

my_custom_rule = rule(
    impl = my_custom_rule_impl,
    attrs = {
        "src": attrs.source(),
        "deps": attrs.list(attrs.dep()),
    },
)
```

```python
# BUCK
load("//rules:my_rules.bzl", "my_custom_rule")

my_custom_rule(
    name = "my_target",
    src = "input.txt",
)
```

**Pros:**
- No prelude modification needed
- Explicit `load()` makes dependencies clear
- Rules live with the project

**Cons:**
- Must use explicit `load()` statements
- Not globally available like prelude rules

### Approach 3: Nix-Backed Prelude Cell (Recommended)

This is Turnkey's recommended approach. The prelude Nix derivation:

1. **Fetches upstream prelude** from buck2-prelude repository
2. **Applies turnkey patches** for customizations
3. **Adds custom rules** from `nix/buck2/prelude-extensions/`

```nix
# nix/buck2/prelude.nix
{ pkgs, lib }:

let
  upstreamPrelude = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2-prelude";
    rev = "...";  # Pinned commit
    hash = "sha256-...";
  };
in
pkgs.runCommand "turnkey-prelude" {} ''
  cp -r ${upstreamPrelude} $out
  chmod -R u+w $out

  # Apply turnkey patches
  patch -d $out -p1 < ${../patches/prelude/nix-integration.patch}

  # Add custom rules
  cp -r ${./prelude-extensions}/* $out/
''
```

**Advantages:**

| Aspect | Extension Cell | Nix-backed Prelude |
|--------|---------------|-------------------|
| Downstream repo size | Adds `prelude-custom/` dir | No additional files |
| Maintenance location | Each downstream repo | Centralized in turnkey |
| Update mechanism | Manual sync | Nix flake update |
| Consistency | Can diverge | All repos use same prelude |

### Approach 4: Forked Prelude

Maintain a fork of the Buck2 prelude with your modifications.

```ini
[external_cells]
prelude = git

[external_cell_prelude]
git_origin = https://github.com/your-org/buck2-prelude-fork.git
commit_hash = your-fork-commit-hash
```

**Pros:**
- Complete control over all rules
- Can modify any prelude behavior

**Cons:**
- Significant maintenance burden
- Must track upstream changes
- Risk of divergence from upstream

## Prelude Version Compatibility

The Buck2 binary and its prelude must be version-matched. Using a mismatched prelude can cause cryptic Starlark errors.

### Symptoms

| Error | Likely Cause | Fix |
|-------|--------------|-----|
| "Unexpected parameter named `X`" | Prelude too new | Use older prelude commit |
| "Missing named-only parameter `X`" | Prelude too old | Use newer prelude commit |

### Finding Compatible Versions

1. Check buck2 version:
   ```bash
   buck2 --version
   # Output: buck2 2025-12-01-75e4243c93877a3db4acf55f20d2e80a32523233
   ```

2. Find matching prelude commit (same date or slightly before):
   ```bash
   curl -s "https://api.github.com/repos/facebook/buck2-prelude/commits?until=2025-12-02T00:00:00Z&per_page=5" | \
     jq -r '.[] | "\(.sha) \(.commit.committer.date)"'
   ```

3. Update `nix/buck2/prelude.nix` with new rev and hash.

### When to Update

Update both buck2 and prelude when:
- nixpkgs updates buck2
- You need new prelude features
- Build errors appear after nixpkgs update

## Existing Extensions

Turnkey includes these prelude extensions:

- `typescript/` - TypeScript compiler integration
- `mdbook/` - Documentation builder

## When to Customize

Consider prelude customization when:

1. **Built-in rules don't support your workflow** - e.g., Nix-specific build patterns
2. **You need enhanced toolchain control** - beyond what system toolchains provide
3. **Platform definitions need modification** - custom constraint values
4. **You're integrating with external systems** - CI/CD, remote execution

## References

- [Buck2 External Cells Documentation](https://buck2.build/docs/users/advanced/external_cells/)
- [Buck2 Writing Rules](https://buck2.build/docs/rule_authors/writing_rules/)
- [Buck2 Writing Toolchains](https://buck2.build/docs/rule_authors/writing_toolchains/)
- [Buck2 Prelude Repository](https://github.com/facebook/buck2-prelude)
