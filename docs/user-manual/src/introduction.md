# Introduction

Turnkey is a toolchain management framework for Nix flakes that simplifies declaring and managing build tools in development environments.

## What is Turnkey?

Turnkey bridges declarative TOML configuration with Nix package resolution, providing:

- **Simple Configuration**: Declare toolchains in `toolchain.toml`
- **Reproducible Environments**: Nix ensures consistent tool versions across machines
- **Incremental Builds**: Fast, cached builds that only rebuild what changed
- **Language Support**: Go, Rust, Python, TypeScript, Solidity, Jsonnet, and more

## Key Features

- Declarative toolchain management via TOML
- Automatic dependency cell generation for the build system
- Native tool wrappers with auto-sync (`go`, `cargo`, `uv`)
- Modular Nix flake integration

## Who Should Use This?

Turnkey is designed for teams who:

- Want reproducible development environments
- Need fast, incremental builds across multiple languages
- Need to manage multiple language toolchains
- Value declarative, version-controlled configuration

## Next Steps

- [Installation](./getting-started/installation.md) - Set up Turnkey in your project
- [Quick Start](./getting-started/quick-start.md) - Build your first project
