# Buck2 Integration

This document describes how Turnkey integrates with Buck2 and covers the architecture of cell generation.

## Overview

Turnkey generates Buck2 cells at shell entry time. Cells are directories that Buck2 treats as separate projects with their own configuration and build rules.

## Cell Resolution

Buck2 cell resolution is **entirely configuration-driven** through `.buckconfig` files. There is no environment variable like `CELL_PATH` for overriding cell locations at runtime.

### Primary Configuration: `[cells]` Section

Cells are defined in `.buckconfig` files:

```ini
[cells]
    root = .
    prelude = path/to/prelude
    toolchains = path/to/toolchains
```

**Key points:**
- Paths are relative to the directory containing the `.buckconfig` file
- Left side: cell alias (alphanumeric + underscores only)
- Right side: filesystem path

### Configuration File Precedence

Buck2 reads configuration from multiple sources (highest to lowest precedence):

1. **Command-line**: `--config`, `--config-file`, `--flagfile`
2. `.buckconfig.local` (repo root)
3. `.buckconfig` (repo root)
4. `.buckconfig.d/` folder (repo root)
5. `~/.buckconfig.local` (user home)
6. `~/.buckconfig.d/` (user home)
7. `/etc/buckconfig` (global)
8. `/etc/buckconfig.d/` (global)

**Reference**: `app/buck2_common/src/legacy_configs/path.rs:35-60`

### Cell Override Restrictions

Buck2 **explicitly bans** overriding cell definitions using the `--config` command-line flag:

```rust
// app/buck2_common/src/legacy_configs/parser.rs:133-144
for banned_section in ["repositories", "cells"] {
    if config_pair.section == banned_section {
        return Err(
            ConfigArgumentParseError::CellOverrideViaCliConfig(banned_section).into(),
        );
    };
}
```

This means:
- `buck2 --config cells.prelude=/path` → **ERROR**
- `buck2 --config repositories.toolchains=/path` → **ERROR**

**Solution**: Use `--config-file` instead, which has no restrictions on `[cells]` sections.

## Generated Cells

### Toolchains Cell (`.turnkey/toolchains/`)

Contains toolchain rules for each declared language:

```python
# Generated rules.star
load("@prelude//toolchains/go:system_go_toolchain.bzl", "system_go_toolchain")

system_go_toolchain(
    name = "go",
    visibility = ["PUBLIC"],
)
```

### Prelude Cell (`.turnkey/prelude/`)

Symlink to Nix-built prelude with:
- Upstream buck2-prelude at pinned commit
- Applied patches
- Custom extensions (TypeScript, mdbook, etc.)

### Dependency Cells

- `godeps/` - Go third-party packages
- `rustdeps/` - Rust crates
- `pydeps/` - Python packages

## Toolchain Mappings

Located at `nix/buck2/mappings.nix`:

```nix
{
  go = {
    skip = false;
    targets = [{
      name = "go";
      rule = "system_go_toolchain";
      load = "@prelude//toolchains/go:system_go_toolchain.bzl";
      visibility = [ "PUBLIC" ];
      dynamicAttrs = registry: {
        go_binary = "${registry.go}/bin/go";
      };
    }];
    implicitDependencies = [ "python" "cxx" ];
    runtimeDependencies = [ ];
  };
}
```

### Mapping Structure

| Field | Description |
|-------|-------------|
| `skip` | Skip this toolchain even if declared |
| `targets` | List of Buck2 targets to generate |
| `implicitDependencies` | Toolchains that must be enabled when this one is |
| `runtimeDependencies` | Packages needed at runtime |
| `dynamicAttrs` | Function to compute attributes from registry |

## Generation Process

1. Devenv shell entry hook runs
2. `nix/devenv/turnkey/buck2.nix` generates toolchains cell
3. Dependency cells built from deps files
4. Symlinks created in `.turnkey/`

## Nix Integration Strategy

Turnkey uses symlinks to Nix store paths for Buck2 cells:

```
.turnkey/
├── toolchains -> /nix/store/...-turnkey-toolchains-cell
├── godeps     -> /nix/store/...-go-deps-cell
├── rustdeps   -> /nix/store/...-rust-deps-cell
├── pydeps     -> /nix/store/...-python-deps-cell
└── prelude    -> /nix/store/...-turnkey-prelude
```

### Config File Solution

Since `--config` can't override cells but `--config-file` can, Turnkey generates a complete `.buckconfig` in the Nix store and symlinks to it:

```ini
# Generated .buckconfig in Nix store
[cells]
    root = .
    prelude = /nix/store/xxx-prelude
    toolchains = /nix/store/yyy-toolchains
    godeps = /nix/store/zzz-godeps
```

This allows each Nix environment to provide its own prelude/toolchains while sharing the same Buck2 project configuration.

### Benefits

- Multiple shells from same git checkout
- Each shell can point to different toolchains/prelude
- No modification of version-controlled files
- Clean integration with Nix's environment management

## Config File Capabilities

### Value Interpolation

Reference other config values:

```ini
[custom]
    base_path = /nix/store/abc123

[cells]
    prelude = $(config custom.base_path)/prelude
    toolchains = $(config custom.base_path)/toolchains
```

**Reference**: `app/buck2_common/src/legacy_configs/parser/resolver.rs:150-153`

### File Includes

Include other configuration files:

```ini
# Required include
<file:path/to/other.buckconfig>

# Optional include (no error if missing)
<?file:path/to/optional.buckconfig>
```

### Limitations

**No Environment Variable Substitution**: Buck2 does **not** support `$(env VAR_NAME)` syntax in config files. Only `$(config section.key)` is supported.

This is why the generated config file approach is necessary.

## External Cells

Buck2 supports external cells (bundled or git-based):

### Bundled External Cells

```ini
[cells]
    prelude = prelude/

[external_cells]
    prelude = bundled
```

### Git External Cells

```ini
[cells]
    prelude = prelude/

[external_cells]
    prelude = git

[external_cell_prelude]
    git_origin = https://github.com/facebook/buck2-prelude.git
    commit_hash = <40-char-sha1-hash>
```

**Note**: External cells don't solve the dynamic path problem since they still require configuration file changes.

## Prelude Customization

The prelude is built by `nix/buck2/prelude.nix`:

1. Fetch upstream buck2-prelude
2. Apply patches from `nix/patches/prelude/`
3. Copy extensions from `nix/buck2/prelude-extensions/`

See [Prelude Extensions](../extending/prelude-extensions.md) for adding custom rules.

## Key Source Files (Buck2)

| Aspect | File Path | Lines |
|--------|-----------|-------|
| Cell Resolution | `app/buck2_core/src/cells.rs` | 1-481 |
| Cell Config Parsing | `app/buck2_common/src/legacy_configs/cells.rs` | 191-530 |
| CLI Argument Parsing | `app/buck2_client_ctx/src/common.rs` | 197-214, 260-338 |
| Config Precedence | `app/buck2_common/src/legacy_configs/configs.rs` | 290-327 |
| Cell Override Ban | `app/buck2_common/src/legacy_configs/parser.rs` | 133-162 |
| Config Value Interpolation | `app/buck2_common/src/legacy_configs/parser/resolver.rs` | 150-216 |
| Prelude Resolution | `app/buck2_interpreter/src/prelude_path.rs` | 41-50 |
| External Cells | `app/buck2_core/src/cells/external.rs` | 1-49 |

## Key Source Files (Turnkey)

| File | Purpose |
|------|---------|
| `nix/buck2/mappings.nix` | Toolchain-to-Buck2 rule mappings |
| `nix/buck2/prelude.nix` | Prelude derivation with patches/extensions |
| `nix/buck2/toolchains-cell.nix` | Toolchains cell generator |
| `nix/buck2/go-deps-cell.nix` | Go dependency cell generator |
| `nix/buck2/rust-deps-cell.nix` | Rust dependency cell generator |
| `nix/devenv/turnkey/buck2.nix` | Devenv integration module |
