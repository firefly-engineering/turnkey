# Shell Environment

Turnkey configures your development shell with all declared toolchains.

## Environment Variables

When entering the shell, Turnkey sets:

- `PATH` - Includes all toolchain binaries
- `TURNKEY_DIRENV_LIB` - Path to direnv integration library

## direnv Integration

For automatic shell activation, use direnv with `.envrc`:

```bash
use flake
```

## Shell Entry Hooks

Turnkey performs these actions on shell entry:

1. Symlinks `.turnkey/prelude` to the prelude cell
2. Symlinks `.turnkey/toolchains` to the generated toolchains
3. Updates dependency cell symlinks if configured
4. Displays welcome message (if configured)

## Verbose Mode

For debugging, set `TURNKEY_VERBOSE=1`:

```bash
TURNKEY_VERBOSE=1 nix develop
```

## Multiple Shells

You can define multiple shells with different toolchains:

```nix
turnkey.toolchains.declarationFiles = {
  default = ./toolchain.toml;
  ci = ./toolchain.ci.toml;
};
```

Access with:
```bash
nix develop .#ci
```
