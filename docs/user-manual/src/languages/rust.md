# Rust Support

Turnkey provides Rust support with automatic dependency management.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
rust = {}
cargo = {}
rustdeps-gen = {}
```

Enable Rust dependencies in `flake.nix`:

```nix
turnkey.toolchains.buck2.rust = {
  enable = true;
  depsFile = ./rust-deps.toml;
  featuresFile = ./rust-features.toml;  # Optional
};
```

## Project Structure

```
my-project/
├── Cargo.toml
├── Cargo.lock
├── rust-deps.toml        # Generated from Cargo.lock
├── rust-features.toml    # Manual feature overrides
└── rust/
    └── mycrate/
        ├── src/
        │   └── lib.rs
        └── rules.star
```

## Build Rules

In `rules.star`:

```python
load("@prelude//rust:rust.bzl", "rust_library", "rust_binary")

rust_library(
    name = "mycrate",
    srcs = glob(["src/**/*.rs"]),
    deps = ["rustdeps//serde:serde"],
)
```

## External Dependencies

Reference crates via the `rustdeps` cell:

```python
deps = [
    "rustdeps//serde:serde",
    "rustdeps//tokio:tokio",
]
```

## Feature Overrides

Use `rust-features.toml` for manual feature control:

```toml
[overrides]
serde = ["derive", "std"]
tokio = ["full"]
```

## Auto-Sync

The `cargo` command is wrapped to auto-sync:

```bash
cargo add serde  # Triggers sync
```
