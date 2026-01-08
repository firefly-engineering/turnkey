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
├── BUCK                  # Root BUCK file with package list
└── vendor/
    └── github.com/
        └── spf13/
            └── cobra/
                ├── BUCK           # go_library target
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
    name = BUCK
```

### Generated BUCK Files

Each package gets a `go_library` target:

```python
# vendor/github.com/spf13/cobra/BUCK
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

### User's BUCK File

```python
# examples/hello-deps/BUCK
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

  # Generate BUCK file content for each dep
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

  # Copy sources and generate BUCK files
  ${lib.concatStrings (lib.mapAttrsToList (name: src: ''
    mkdir -p $out/vendor/${name}
    cp -r ${src}/* $out/vendor/${name}/
    cat > $out/vendor/${name}/BUCK <<'BUCK'
    ${generateBuck name src}
    BUCK
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

Use `gomod2nix` to generate a Nix expression from go.mod, then convert to BUCK files.

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
