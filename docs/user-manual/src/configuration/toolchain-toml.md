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

## Available Toolchains

### Build Systems
- `buck2` - Buck2 build system

### Languages
- `go` - Go compiler and tools
- `rust` - Rust compiler (rustc)
- `cargo` - Cargo package manager
- `python` - Python interpreter
- `nodejs` - Node.js runtime
- `typescript` - TypeScript compiler

### Dependency Tools
- `godeps-gen` - Generate go-deps.toml from go.mod
- `rustdeps-gen` - Generate rust-deps.toml from Cargo.lock
- `pydeps-gen` - Generate python-deps.toml

### Development Tools
- `nix` - Nix package manager
- `reindeer` - Rust dependency generator

## Toolchain Options

Currently, toolchains are declared with empty options:

```toml
[toolchains]
go = {}
```

Future versions may support version pinning and other options.

## Custom Registries

The registry mapping toolchain names to packages can be customized in your `flake.nix`. See [Buck2 Integration](./buck2-integration.md) for details.
