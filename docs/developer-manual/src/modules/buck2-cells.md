# Buck2 Cell Generation

This document describes how Turnkey generates Buck2 cells from Nix derivations.

## Overview

Turnkey generates several types of Buck2 cells:
- **Toolchains cell** - Language toolchains (Go, Rust, Python, etc.)
- **Dependency cells** - Third-party packages (godeps, rustdeps, pydeps)
- **Prelude cell** - Buck2 prelude with extensions

All cells are built as Nix derivations and symlinked into `.turnkey/`.

## Toolchains Cell

Located at `nix/buck2/toolchains-cell.nix`. Generated from `nix/buck2/mappings.nix`.

### Mapping Structure

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

### Generated Output

`rules.star` is generated with:

1. Load statements for each rule
2. Rule instantiations with configured attributes
3. Visibility set to PUBLIC

### Adding Toolchain Mappings

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

## Go Dependency Cell

Built by `nix/buck2/go-deps-cell.nix`.

### Cell Structure

```
/nix/store/<hash>-go-deps-cell/
├── .buckconfig           # Cell identity
├── rules.star            # Root rules.star file with package list
└── vendor/
    └── github.com/
        └── spf13/
            └── cobra/
                ├── rules.star    # go_library target
                └── *.go          # Source files from Nix
```

The directory structure mirrors Go import paths, matching Buck2's conventions for third-party Go packages.

### Cell Configuration

```ini
# .buckconfig
[cells]
    godeps = .
    prelude = bundled://

[buildfile]
    name = rules.star
```

### Generated rules.star Files

Each package gets a `go_library` target:

```python
# vendor/github.com/spf13/cobra/rules.star
go_library(
    name = "cobra",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    importpath = "github.com/spf13/cobra",
    deps = [
        "//vendor/github.com/spf13/pflag:pflag",
        "//vendor/github.com/inconshreveable/mousetrap:mousetrap",
    ],
    visibility = ["PUBLIC"],
)
```

### Target Path Format

When writing rules.star files that depend on packages from the godeps cell:

```
godeps//vendor/<import-path>:<target-name>
```

Where:
- `godeps//` - the cell alias (configured in .buckconfig)
- `vendor/` - **required prefix** - all packages live under vendor/
- `<import-path>` - the full Go import path
- `<target-name>` - the **directory name** (last path component), NOT the package name

**Examples:**

| Go Import | Correct Buck2 Target | Why |
|-----------|---------------------|-----|
| `github.com/spf13/cobra` | `godeps//vendor/github.com/spf13/cobra:cobra` | Target is `cobra` (dir name) |
| `github.com/pelletier/go-toml/v2` | `godeps//vendor/github.com/pelletier/go-toml/v2:v2` | Target is `v2` (dir name) |
| `golang.org/x/sys/unix` | `godeps//vendor/golang.org/x/sys/unix:unix` | Target is `unix` (dir name) |

**Common Mistakes:**

```python
# WRONG - missing vendor/ prefix
deps = ["godeps//github.com/spf13/cobra:cobra"]

# WRONG - using package name instead of directory name for versioned imports
deps = ["godeps//vendor/github.com/pelletier/go-toml/v2:go-toml"]

# CORRECT
deps = ["godeps//vendor/github.com/pelletier/go-toml/v2:v2"]
```

### Import Path Resolution

Buck2's `importpath` attribute ensures the Go compiler sees the correct import path:
- Buck2 target: `godeps//vendor/github.com/spf13/cobra:cobra`
- Go import: `import "github.com/spf13/cobra"`

The `go_library` rule's `importpath = "github.com/spf13/cobra"` makes this work.

### Nix Integration

```nix
# nix/buck2/go-deps-cell.nix
{ pkgs, lib, goDepsFile }:

let
  # Parse go-deps.toml to get dependencies
  deps = builtins.fromTOML (builtins.readFile goDepsFile);

  # Fetch each dependency source
  depSources = lib.mapAttrs (name: info:
    pkgs.fetchFromGitHub {
      owner = info.owner;
      repo = info.repo;
      rev = info.rev;
      hash = info.hash;
    }
  ) deps.deps;

  # Generate rules.star file content for each dep
  generateBuck = name: src: ''
    go_library(
      name = "${lib.last (lib.splitString "/" name)}",
      srcs = glob(["*.go"], exclude = ["*_test.go"]),
      importpath = "${name}",
      deps = [${formatDeps (getDeps name)}],
      visibility = ["PUBLIC"],
    )
  '';
in
pkgs.runCommand "go-deps-cell" {} ''
  mkdir -p $out/vendor

  ${lib.concatStrings (lib.mapAttrsToList (name: src: ''
    mkdir -p $out/vendor/${name}
    cp -r ${src}/* $out/vendor/${name}/
    cat > $out/vendor/${name}/rules.star <<'EOF'
    ${generateBuck name src}
    EOF
  '') depSources)}

  # Generate cell .buckconfig
  cat > $out/.buckconfig <<'EOF'
  [cells]
      godeps = .
  EOF
''
```

## Rust Dependency Cell

Built by `nix/buck2/rust-deps-cell.nix`.

### Process

1. Reads rust-deps.toml
2. Fetches crates from crates.io
3. Computes unified features across dependency graph
4. Generates rules.star with features and deps

### Special Handling

Rust crates may require:
- **rustc flags** - Build scripts that emit `cargo:rustc-cfg`
- **Generated files** - Build scripts that generate `.rs` files
- **Native code** - Build scripts that compile C/assembly

See [Dependency Generators](../extending/dependency-generators.md) for handling these cases.

## Python Dependency Cell

Built by `nix/buck2/python-deps-cell.nix`.

### Process

1. Reads python-deps.toml
2. Fetches wheels from PyPI
3. Generates rules.star per package

## Cell Configuration

Each cell gets a `.buckconfig`:

```ini
[cells]
    cellname = .
    prelude = path/to/prelude

[buildfile]
    name = rules.star
```

## Dual-Build Compatibility

A key design goal is that code builds with both native tools and Buck2:

```bash
# Native build (uses go.mod directly)
go build ./...

# Buck2 build (uses generated cell)
buck2 build //...
```

This is achieved by:

1. **No import rewriting** - Go code uses standard import paths (`github.com/foo/bar`)
2. **importpath attribute** - Buck2's `go_library` rule's `importpath` tells the compiler the correct path
3. **Nix-managed deps** - Dependencies fetched by Nix, not vendored in repo

## Adding New Dependency Cell Types

To add support for a new language:

1. **Create deps generator** (e.g., `newlang-deps-gen`)
2. **Create cell builder** (`nix/buck2/newlang-deps-cell.nix`)
3. **Add to devenv module** (`nix/devenv/turnkey/buck2.nix`)
4. **Add configuration options** for deps file path

## Debugging

### Inspect Generated rules.star Files

```bash
cat .turnkey/godeps/vendor/github.com/spf13/cobra/rules.star
```

### Check Cell Contents

```bash
ls -la .turnkey/godeps/vendor/
```

### Verify Cell Configuration

```bash
cat .turnkey/godeps/.buckconfig
```

### List Available Targets

```bash
buck2 targets godeps//...
```
