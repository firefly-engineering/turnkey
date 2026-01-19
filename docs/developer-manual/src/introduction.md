# Introduction

This manual is for developers who want to understand, extend, or contribute to Turnkey.

## What You'll Learn

- Turnkey's architecture and design principles
- How the Nix module system works
- Adding new toolchains and language support
- Creating custom Buck2 rules
- Contributing to the project

## Prerequisites

You should be familiar with:

- **Nix** - Flakes, derivations, module system
- **Buck2** - Targets, rules, toolchains
- **Starlark** - Buck2's configuration language

## Project Philosophy

Turnkey follows these principles:

1. **Simplicity over features** - Solve common cases elegantly
2. **Declarative configuration** - TOML in, working environment out
3. **Reproducibility** - Same inputs = same outputs, always
4. **Composition** - Build complex systems from simple parts
5. **Transparency** - Generated code should be readable

## Repository Structure

```
turnkey/
├── nix/
│   ├── flake-parts/turnkey/  # Flake-parts integration
│   ├── devenv/turnkey/       # Devenv shell configuration
│   ├── registry/             # Default toolchain registry
│   ├── buck2/                # Buck2 cell generation
│   │   ├── prelude.nix       # Prelude derivation
│   │   ├── mappings.nix      # Toolchain mappings
│   │   └── prelude-extensions/
│   ├── packages/             # Tool packages
│   └── patches/              # Upstream patches
├── cmd/                      # CLI tools (Go)
├── docs/                     # Documentation
└── examples/                 # Example projects
```
