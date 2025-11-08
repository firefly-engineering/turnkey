# Buck2 Cell Resolution and Dynamic Configuration

This document describes how Buck2 resolves cells and provides solutions for dynamically configuring cell paths (e.g., for Nix integration).

## Overview

Buck2 cell resolution is **entirely configuration-driven** through `.buckconfig` files. There is no environment variable like `CELL_PATH` for overriding cell locations at runtime.

## Cell Configuration Basics

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

## The Problem: Cell Overrides Are Banned via `--config`

Buck2 **explicitly bans** overriding cell definitions using the `--config` command-line flag:

```rust
// app/buck2_common/src/legacy_configs/parser.rs:133-144
pub(crate) fn apply_config_arg(
    &mut self,
    config_pair: &ResolvedConfigFlag,
    current_cell: &CellRootPath,
) -> buck2_error::Result<()> {
    for banned_section in ["repositories", "cells"] {
        if config_pair.section == banned_section {
            return Err(
                ConfigArgumentParseError::CellOverrideViaCliConfig(banned_section).into(),
            );
        };
    }
    // ...
}
```

**This means:**
- ❌ `buck2 --config cells.prelude=/path` → **ERROR**
- ❌ `buck2 --config repositories.toolchains=/path` → **ERROR**

**Test confirmation**: `tests/core/client/test_common_opts.py:39-42`

## ✅ Solution: Use `--config-file` Instead

The ban on cell overrides **only applies to `--config` flags**, NOT to `--config-file`.

### Why This Works

The `--config-file` path uses `parse_file()` which has no restrictions:

```rust
// app/buck2_common/src/legacy_configs/parser.rs:115-131
pub(crate) async fn parse_file(
    &mut self,
    path: &ConfigPath,
    source: Option<Location>,
    follow_includes: bool,
    file_ops: &mut dyn ConfigParserFileOps,
) -> buck2_error::Result<()> {
    // No restrictions on [cells] or [repositories] sections! ✅
}
```

### Direct Usage

Create a config file with cell definitions:

```ini
# custom-cells.buckconfig
[cells]
    prelude = /nix/store/...-prelude
    toolchains = /nix/store/...-toolchains
```

Then use it:

```bash
buck2 --config-file custom-cells.buckconfig build //my:target
```

## Recommended Approach: Wrapper Script

For dynamic configuration based on environment variables (ideal for Nix):

```bash
#!/usr/bin/env bash
# buck2-nix-wrapper.sh

# Read environment variables (or use defaults)
PRELUDE_PATH="${BUCK2_PRELUDE_PATH:-prelude}"
TOOLCHAINS_PATH="${BUCK2_TOOLCHAINS_PATH:-toolchains}"

# Create temporary buckconfig
TEMP_CONFIG=$(mktemp)
trap "rm -f $TEMP_CONFIG" EXIT

cat > "$TEMP_CONFIG" <<EOF
[cells]
    prelude = $PRELUDE_PATH
    toolchains = $TOOLCHAINS_PATH
EOF

# Run buck2 with the generated config file
exec buck2 --config-file "$TEMP_CONFIG" "$@"
```

### Usage Example

```bash
# Shell 1 - using Nix-managed prelude A
export BUCK2_PRELUDE_PATH=/nix/store/abc123-prelude-1.0
./buck2-nix-wrapper.sh build //my:target

# Shell 2 - using Nix-managed prelude B
export BUCK2_PRELUDE_PATH=/nix/store/def456-prelude-2.0
./buck2-nix-wrapper.sh build //my:target
```

### Benefits

- ✅ Multiple shells from same git checkout
- ✅ Each shell can point to different toolchains/prelude
- ✅ No modification of version-controlled files
- ✅ Clean integration with Nix's environment management

## Config File Capabilities

`--config-file` supports the full `.buckconfig` syntax:

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

The regex pattern: `\$\(config ([^)]*)\)`

### File Includes

Include other configuration files:

```ini
# Required include
<file:path/to/other.buckconfig>

# Optional include (no error if missing)
<?file:path/to/optional.buckconfig>
```

Paths can be:
- Relative to the including file (recommended)
- Absolute paths from filesystem root

**Reference**: `docs/concepts/buckconfig.md:179-228`

### Character Encoding

Support for escape sequences:

- `\\` - backslash
- `\"` - double quote
- `\n` - newline
- `\r` - carriage return
- `\t` - tab
- `\x##` - Unicode character (2-digit hex)
- `\u####` - Unicode character (4-digit hex)
- `\U########` - Unicode character (8-digit hex)

**Reference**: `docs/concepts/buckconfig.md:54-66`

## Limitations

### No Environment Variable Substitution

Buck2 does **not** support `$(env VAR_NAME)` syntax in config files. Only `$(config section.key)` is supported.

**Evidence**: No environment variable expansion found in:
- `app/buck2_common/src/legacy_configs/parser/resolver.rs`
- `app/buck2_common/src/legacy_configs/configs.rs`

This is why the wrapper script approach is necessary.

### Testing-Only Environment Variables

There are some Buck2 environment variables, but they're testing-only:

```rust
// app/buck2_common/src/legacy_configs/cells.rs:534-590
buck2_env!("BUCK2_TEST_SKIP_DEFAULT_EXTERNAL_CONFIG", bool, applicability = testing)
buck2_env!("BUCK2_TEST_EXTRA_EXTERNAL_CONFIG", applicability = testing)
```

These are **not available** in production builds.

## External Cells

Buck2 also supports external cells (bundled or git-based), but these have different use cases:

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

**Reference**: `app/buck2_core/src/cells/external.rs:1-49`

**Note**: External cells don't solve the dynamic path problem since they still require configuration file changes.

## Nix Integration Strategy

For Nix integration, the recommended approach is:

1. **Create a Nix derivation** that provides a wrapper script
2. **Script reads Nix-provided paths** from environment variables
3. **Script generates temporary `.buckconfig`** with cell definitions
4. **Script invokes buck2** with `--config-file` pointing to temp config

### Example Nix Derivation Structure

```nix
{ buck2, prelude, toolchains }:

writeShellScriptBin "buck2" ''
  export BUCK2_PRELUDE_PATH="${prelude}"
  export BUCK2_TOOLCHAINS_PATH="${toolchains}"

  TEMP_CONFIG=$(mktemp)
  trap "rm -f $TEMP_CONFIG" EXIT

  cat > "$TEMP_CONFIG" <<EOF
[cells]
    prelude = $BUCK2_PRELUDE_PATH
    toolchains = $BUCK2_TOOLCHAINS_PATH
EOF

  exec ${buck2}/bin/buck2 --config-file "$TEMP_CONFIG" "$@"
''
```

This allows each Nix environment to provide its own prelude/toolchains while sharing the same Buck2 project configuration.

## Key Source Files

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
| Config File Precedence | `app/buck2_common/src/legacy_configs/path.rs` | 35-60 |

## User Documentation

- `.buckconfig` format: `docs/concepts/buckconfig.md`
- Cell configuration section: lines 252-277
- Config precedence: lines 156-173
- Value interpolation: lines 92-105
- File includes: lines 179-228

## Summary

1. ❌ Cannot use `--config cells.foo=bar` (explicitly banned)
2. ✅ Can use `--config-file` with full cell definitions
3. ✅ Wrapper script pattern works perfectly for dynamic paths
4. ✅ Ideal for Nix integration where paths are managed externally
5. ⚠️ No native environment variable substitution in config files
