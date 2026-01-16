# TypeScript Rules Design for Buck2

## Overview

This document describes the design for TypeScript support in turnkey's Buck2 prelude.

## Goals

1. **Simple compilation**: Compile TypeScript to JavaScript using `tsc`
2. **Type checking**: Generate `.d.ts` declaration files
3. **Runnable binaries**: Execute compiled code with Node.js
4. **Hermetic builds**: All dependencies fetched via Nix, no network access during build

## Non-Goals (for initial implementation)

- Custom transpilers (esbuild, swc) - use tsc only
- Incremental compilation / worker mode
- Project references
- npm package resolution within Buck2 (deps come from Nix)

## Architecture

### Toolchain

```starlark
TypeScriptToolchainInfo = provider(
    fields = {
        "node": provider_field(RunInfo),         # Node.js binary
        "tsc": provider_field(RunInfo),          # TypeScript compiler
        "tsc_flags": provider_field(list[str]),  # Default tsc flags
    },
)
```

The toolchain is configured via `system_typescript_toolchain()` which reads paths from the Nix-provided environment.

### Rules

#### typescript_library

Compiles TypeScript sources to JavaScript and declaration files.

```python
typescript_library(
    name = "mylib",
    srcs = ["src/index.ts", "src/utils.ts"],
    deps = [":other_lib"],  # Other typescript_library targets
    tsconfig = "tsconfig.json",  # Optional, uses sensible defaults
)
```

**Outputs:**
- `*.js` files in output directory
- `*.d.ts` declaration files
- Source maps (optional)

**Implementation:**
1. Collect all source files and transitive dependencies
2. Generate or use provided tsconfig.json
3. Run `tsc --outDir <output> --declaration`
4. Return `TypeScriptLibraryInfo` provider with outputs

#### typescript_binary

Creates a runnable Node.js application from TypeScript.

```python
typescript_binary(
    name = "myapp",
    main = "src/main.ts",
    srcs = ["src/main.ts"],
    deps = [":mylib"],
)
```

**Outputs:**
- Compiled JavaScript
- Wrapper script that runs `node <main.js>`

#### typescript_test

Runs TypeScript tests (initially with a simple runner, later with Jest/Vitest support).

```python
typescript_test(
    name = "mylib_test",
    srcs = ["test/mylib.test.ts"],
    deps = [":mylib"],
)
```

### Providers

```starlark
TypeScriptLibraryInfo = provider(
    fields = {
        "output_dir": provider_field(Artifact),     # Directory with .js files
        "declaration_dir": provider_field(Artifact), # Directory with .d.ts files
        "transitive_deps": provider_field(Tset),     # All transitive TypeScript deps
    },
)
```

## File Structure

```
nix/buck2/prelude-extensions/
└── typescript/
    ├── toolchain.bzl      # TypeScriptToolchainInfo, system_typescript_toolchain
    ├── providers.bzl      # TypeScriptLibraryInfo
    ├── typescript.bzl     # Rule registration, extra_attributes
    ├── ts_library.bzl     # typescript_library implementation
    ├── ts_binary.bzl      # typescript_binary implementation
    └── ts_test.bzl        # typescript_test implementation
```

## Toolchain Configuration

The toolchain uses Nix-provided Node.js and TypeScript. In `toolchain.toml`:

```toml
[toolchains]
nodejs = {}
typescript = {}
```

The turnkey module adds these to the registry and generates the toolchain target.

## Dependency Management

TypeScript dependencies (npm packages) are handled outside Buck2:
1. User maintains `package.json` and `package-lock.json`
2. Nix fetches and builds npm packages
3. Packages are available in `node_modules/` (symlinked from Nix store)
4. Buck2 rules reference installed packages via tsconfig paths

This follows the same pattern as Go (go.mod → godeps cell) and Rust (Cargo.lock → rustdeps cell).

Future work: Create a `tsdeps` cell that pre-fetches npm packages.

## Example Usage

```python
# BUCK file

load("@prelude//typescript:typescript.bzl", "typescript_library", "typescript_binary")

typescript_library(
    name = "utils",
    srcs = glob(["src/utils/**/*.ts"]),
)

typescript_library(
    name = "core",
    srcs = glob(["src/core/**/*.ts"]),
    deps = [":utils"],
)

typescript_binary(
    name = "cli",
    main = "src/cli/main.ts",
    srcs = glob(["src/cli/**/*.ts"]),
    deps = [":core"],
)
```

## Implementation Phases

### Phase 1: Basic Compilation
- [ ] `TypeScriptToolchainInfo` provider
- [ ] `system_typescript_toolchain` rule
- [ ] `typescript_library` with basic tsc invocation
- [ ] Integration with turnkey toolchain mappings

### Phase 2: Binaries and Tests
- [ ] `typescript_binary` rule
- [ ] `typescript_test` rule with basic runner
- [ ] E2E test fixture

### Phase 3: Dependency Cell (Future)
- [ ] `tsdeps-gen` tool (like godeps-gen)
- [ ] npm package prefetching
- [ ] `tsdeps` cell generation

## References

- [Aspect rules_ts](https://github.com/aspect-build/rules_ts) - Bazel TypeScript rules
- [Buck2 Writing Rules](https://buck2.build/docs/rule_authors/writing_rules/)
- [Buck2 Go Toolchain](https://github.com/facebook/buck2-prelude/tree/main/go)
