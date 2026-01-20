# toolchain.toml

The `toolchain.toml` file declares which toolchains your project needs.

## Basic Structure

```toml
[toolchains]
buck2 = {}
go = {}
rust = {}
python = {}
```

Each key under `[toolchains]` is a toolchain name that will be resolved from the registry.

## Version Pinning

You can pin specific versions when the registry provides multiple versions:

```toml
[toolchains]
go = { version = "1.22" }      # Pin to Go 1.22
python = { version = "3.11" }  # Pin to Python 3.11
rust = {}                       # Use registry default
```

If no version is specified, the registry's default version is used.

## Available Toolchains

### Build Systems
- `buck2` - Buck2 build system

### Languages
- `go` - Go compiler and tools
- `rust` - Rust compiler (rustc)
- `cargo` - Cargo package manager
- `clippy` - Rust linter
- `rustfmt` - Rust formatter
- `rust-analyzer` - Rust LSP server
- `python` - Python interpreter
- `uv` - Python package manager
- `ruff` - Python linter and formatter
- `nodejs` - Node.js runtime
- `typescript` - TypeScript compiler
- `biome` - Fast linter/formatter for JS/TS/JSON

### Solidity
- `solc` - Solidity compiler
- `foundry` - Ethereum dev toolkit (forge, cast, anvil)

### Other Tools
- `nix` - Nix package manager
- `reindeer` - Rust Buck2 target generator
- `jsonnet` - Jsonnet to JSON compiler
- `mdbook` - Documentation tool
- `tk` - Turnkey CLI wrapper for buck2

## Internal Tools

Dependency generators (`godeps-gen`, `rustdeps-gen`, `pydeps-gen`, `jsdeps-gen`, `soldeps-gen`) are **automatically included** when their corresponding language is enabled. You don't need to list them in `toolchain.toml`.

For example, if you have `go = {}` in your toolchain.toml and `buck2.go.enable = true` in your flake.nix, `godeps-gen` will automatically be available in your shell.

## Example Configurations

### Minimal Go Project

```toml
[toolchains]
buck2 = {}
go = {}
```

### Full-Stack Project

```toml
[toolchains]
# Build
buck2 = {}

# Backend
go = {}
python = {}

# Frontend
nodejs = {}
typescript = {}
biome = {}

# Development
nix = {}
```

### Pinned Versions

```toml
[toolchains]
go = { version = "1.22" }
python = { version = "3.11" }
nodejs = { version = "20" }
rust = { version = "1.75" }
```

## Custom Registries

The registry mapping toolchain names to packages can be customized in your `flake.nix`. See [Registry Pattern](../../../developer-manual/src/architecture/registry.md) for details on:

- Adding custom toolchains via `registryExtensions`
- Creating reusable registry overlays
- Multi-version toolchain support
