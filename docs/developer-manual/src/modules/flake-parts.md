# Flake-Parts Module

Located at `nix/flake-parts/turnkey/default.nix`.

## Purpose

Provides the user-facing API for Turnkey configuration in flakes.

## Options Reference

### turnkey.toolchains.enable

```nix
type = types.bool;
default = true;
```

Enable/disable Turnkey toolchain management.

### turnkey.toolchains.declarationFiles

```nix
type = types.attrsOf types.path;
default = {};
```

Map shell names to toolchain.toml files:

```nix
declarationFiles = {
  default = ./toolchain.toml;
  ci = ./toolchain.ci.toml;
};
```

### turnkey.toolchains.registry

```nix
type = types.lazyAttrsOf types.package;
default = {};
```

Custom toolchain registry. If empty, uses default registry.

### turnkey.toolchains.wrapNativeTools

```nix
type = types.bool;
default = true;
```

Wrap `go`, `cargo`, `uv` with auto-sync behavior.

### turnkey.toolchains.buck2

Nested options for Buck2 integration:

- `enable` - Enable Buck2 cell generation
- `prelude.strategy` - How to provide prelude ("nix", "bundled", "git", "path")
- `go.enable`, `go.depsFile` - Go dependency configuration
- `rust.enable`, `rust.depsFile` - Rust dependency configuration
- `python.enable`, `python.depsFile` - Python dependency configuration

## Implementation

The module:

1. Imports default registry
2. Builds tw wrappers for native tools
3. Creates shell configurations for each declaration file
4. Passes configuration to devenv module

## Extending

Add new options in the `options.perSystem` block and implement in `config.perSystem`.
