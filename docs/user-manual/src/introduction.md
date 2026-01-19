# Introduction

Turnkey is a toolchain management framework for Nix flakes that simplifies declaring and managing build tools in development environments.

## What is Turnkey?

Turnkey bridges declarative TOML configuration with Nix package resolution, providing:

- **Simple Configuration**: Declare toolchains in `toolchain.toml`
- **Reproducible Environments**: Nix ensures consistent tool versions across machines
- **Buck2 Integration**: First-class support for Buck2 build system
- **Language Support**: Go, Rust, Python, TypeScript, and more

## Key Features

- Declarative toolchain management via TOML
- Automatic dependency cell generation for Buck2
- Native tool wrappers with auto-sync (`go`, `cargo`, `uv`)
- Modular Nix flake integration

## Who Should Use This?

Turnkey is designed for teams who:

- Want reproducible development environments
- Use or are adopting Buck2 as their build system
- Need to manage multiple language toolchains
- Value declarative, version-controlled configuration

## Next Steps

- [Installation](./getting-started/installation.md) - Set up Turnkey in your project
- [Quick Start](./getting-started/quick-start.md) - Build your first project
