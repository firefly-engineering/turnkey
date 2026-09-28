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

## Features

Each vendored crate is built with the features Cargo would give it. `tk sync`
records your workspace members' dependency specs (version, `features`,
`default-features`) in `rust-deps.toml` as `[[requested]]` entries, and the
rustdeps cell resolves features from them the way Cargo does:

- a crate's default features are on only if some dependent asks for them
  (it does not set `default-features = false`);
- an optional dependency is part of the build only when an enabled feature
  activates it;
- when several versions of a crate are vendored, each gets its own features,
  and a dependency resolves to the version its requirement matches.

Declare what you need in `Cargo.toml` as you would for Cargo; there is
nothing to repeat for Buck2.

`tk sync` also records each vendored crate's **package slice** in
`rust-deps.toml`: its features and dependencies as Cargo resolves them for
`cargo test --workspace`, on each of the project's platforms (`cargo tree`,
one run per platform). The cell does not use the slices yet. Recording them
means that:

- `tk sync` runs `cargo` with `--locked`, so `Cargo.lock` must match
  `Cargo.toml`. After editing a manifest by hand, run a cargo command (or
  `tw cargo …`) that updates the lock. The first run downloads the crates
  into `~/.cargo/registry`.
- Every workspace member's `Cargo.toml` is a source of `rust-deps.toml`, so a
  features-only edit in a member regenerates it.

## Feature Overrides

Use `rust-features.toml` where the result must differ from Cargo's:

```toml
[overrides]
serde = ["derive", "std"]          # Complete replacement
tokio = { add = ["full"] }         # Requested on top, with what it enables
inotify = { remove = ["stream"] }  # Never enabled, nor what only it enables
```

Overrides are keyed by crate name and apply to every vendored version.
Removing `"default"` drops the crate's default feature set.

## Auto-Sync

The `cargo` command is wrapped to auto-sync:

```bash
cargo add serde  # Triggers sync
```
