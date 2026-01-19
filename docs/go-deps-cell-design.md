# Nix-Managed Go Dependencies Cell Design

## Overview

This document describes the architecture for managing Go external dependencies through a Nix-generated Buck2 cell, enabling builds that work with both `go build` and `buck2 build` without import path rewriting.

## Goals

1. **Dual-build compatibility**: Code builds with native `go build` and `buck2 build`
2. **No import rewriting**: Go code uses standard import paths (`github.com/foo/bar`)
3. **Nix-managed deps**: Dependencies fetched and built by Nix, not vendored in repo
4. **Buck2 integration**: Generated cell provides `go_library` targets for Buck2

## Architecture

### Cell Structure

```
/nix/store/<hash>-go-deps-cell/
├── .buckconfig           # Cell identity
├── rules.star                  # Root rules.star file with package list
└── vendor/
    └── github.com/
        └── spf13/
            └── cobra/
                ├── rules.star           # go_library target
                └── *.go           # Source files from Nix
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

### User's rules.star File

```python
# examples/hello-deps/rules.star
go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/spf13/cobra:cobra",
    ],
)
```

### Import Path Resolution

Buck2's `importpath` attribute ensures the Go compiler sees the correct import path:
- Buck2 target: `godeps//vendor/github.com/spf13/cobra:cobra`
- Go import: `import "github.com/spf13/cobra"`

The `go_library` rule's `importpath = "github.com/spf13/cobra"` makes this work.

### Target Path Format Reference

When writing rules.star files that depend on packages from the godeps cell, use this format:

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
| `github.com/pelletier/go-toml/v2` | `godeps//vendor/github.com/pelletier/go-toml/v2:v2` | Target is `v2` (dir name), not `go-toml` |
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

**Why the target name is the directory name:**

Buck2 targets are named after the directory containing the rules.star file. For versioned Go modules like `go-toml/v2`, the rules.star file lives in the `v2/` directory, so the target is named `v2`, not `go-toml`. The `importpath` attribute in the generated rules.star file tells the Go compiler to use the correct import path.

## Nix Integration

### Derivation Structure

```nix
# nix/buck2/go-deps.nix
{ pkgs, lib, goMod, goSum }:

let
  # Parse go.mod to get dependencies
  deps = parseGoMod goMod goSum;

  # Fetch each dependency source
  depSources = mapAttrs (name: version:
    pkgs.fetchFromGitHub { /* ... */ }
    # or use gomod2nix for proper vendoring
  ) deps;

  # Generate rules.star file content for each dep
  generateBuck = name: src: ''
    go_library(
      name = "${baseName name}",
      srcs = glob(["*.go"], exclude = ["*_test.go"]),
      importpath = "${name}",
      deps = [${formatDeps (getDeps name)}],
      visibility = ["PUBLIC"],
    )
  '';
in
pkgs.runCommand "go-deps-cell" {} ''
  mkdir -p $out/vendor

  # Copy sources and generate rules.star files
  ${lib.concatStrings (lib.mapAttrsToList (name: src: ''
    mkdir -p $out/vendor/${name}
    cp -r ${src}/* $out/vendor/${name}/
    cat > $out/vendor/${name}/rules.star <<'rules.star'
    ${generateBuck name src}
    rules.star
  '') depSources)}

  # Generate cell .buckconfig
  cat > $out/.buckconfig <<'EOF'
  [cells]
      godeps = .
  EOF
''
```

### Integration with buck2.nix

The deps cell would be generated alongside the toolchains cell:

```nix
# In buck2.nix config section
goDepsCell = if cfg.go.enable then
  import ./go-deps.nix {
    inherit pkgs lib;
    goMod = cfg.go.modFile;
    goSum = cfg.go.sumFile;
  }
else null;
```

### .buckconfig Integration

```ini
# Generated .buckconfig
[cells]
    root = .
    toolchains = /nix/store/<hash>-turnkey-toolchains-cell
    godeps = /nix/store/<hash>-go-deps-cell
    prelude = prelude

[external_cells]
    prelude = bundled
```

## Implementation Phases

### Phase 1: Simple Case (No Transitive Deps)

1. Create `examples/hello-deps/` with go.mod importing one package
2. Manually generate the deps cell derivation
3. Verify `go build` and `buck2 build` both work
4. Document the approach

### Phase 2: Transitive Dependencies

1. Parse full dependency graph from go.sum
2. Generate all transitive deps with correct `deps` attributes
3. Handle diamond dependencies
4. Test with a real-world package like `cobra`

### Phase 3: Automation

1. Create `nix/buck2/go-deps.nix` module
2. Add configuration options to buck2.nix
3. Auto-generate deps cell when go.mod changes
4. Symlink to `.turnkey/godeps` like toolchains cell

## Alternative Approaches Considered

### 1. gomod2nix Integration

Use `gomod2nix` to generate a Nix expression from go.mod, then convert to rules.star files.

**Pros**: Mature tooling, handles vendoring correctly
**Cons**: Extra layer of abstraction

### 2. gobuckify in Nix

Run Buck2's `gobuckify` inside a Nix derivation.

**Pros**: Uses Buck2's native tooling
**Cons**: Requires Go toolchain at eval time, complex setup

### 3. Prebuilt Archives

Build Go packages as archives in Nix, reference via `prebuilt_go_library`.

**Pros**: Faster builds (already compiled)
**Cons**: Buck2's `prebuilt_go_library` support is limited

## Open Questions

1. How to handle replace directives in go.mod?
2. Should we support private modules (GOPRIVATE)?
3. How to handle cgo dependencies in third-party packages?
4. Should the deps cell be per-project or shared across projects?

## References

- [Buck2 Go Overview](https://buck2.build/docs/users/languages/go/overview/)
- [Buck2 Third-Party Packages](https://buck2.build/docs/users/languages/go/third_party_packages/)
- [gomod2nix](https://github.com/nix-community/gomod2nix)
- [Nix buildGoModule](https://nixos.org/manual/nixpkgs/stable/#sec-language-go)
