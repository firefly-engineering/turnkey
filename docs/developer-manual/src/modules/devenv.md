# Devenv Module

Located at `nix/devenv/turnkey/default.nix`.

## Purpose

Configures individual devenv shells with toolchains and Buck2 integration.

## Options

### turnkey.enable

Enable Turnkey for this shell.

### turnkey.declarationFile

Path to toolchain.toml file.

### turnkey.registry

Package registry (usually inherited from flake-parts).

## How It Works

1. **Parse TOML**: Reads toolchain.toml

```nix
toolchainDeclaration = builtins.fromTOML (builtins.readFile cfg.declarationFile);
toolchainNames = builtins.attrNames toolchainDeclaration.toolchains;
```

2. **Resolve packages**: Maps names to packages

```nix
resolvedPackages = map (name: cfg.registry.${name}) toolchainNames;
```

3. **Add to shell**: Packages added to devenv

```nix
config.packages = resolvedPackages;
```

## Sub-Module: buck2.nix

The buck2.nix sub-module (`nix/devenv/turnkey/buck2.nix`) handles:

- Toolchains cell generation
- Prelude cell symlink
- Dependency cell symlinks
- Shell entry hooks

## Shell Entry Hooks

Devenv's `enterShell` hook:

1. Symlinks `.turnkey/prelude` → Nix store
2. Symlinks `.turnkey/toolchains` → Nix store
3. Symlinks dependency cells if configured
4. Displays welcome message

## Debugging

Enable verbose output:

```bash
TURNKEY_VERBOSE=1 nix develop
```
