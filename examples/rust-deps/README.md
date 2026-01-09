# Rust External Dependencies Example

This example demonstrates using external Rust crates (from crates.io)
with Buck2 through the turnkey framework.

## Current Status

**Not yet functional** - requires reindeer integration.

## Required Setup

Buck2 uses [reindeer](https://github.com/facebookincubator/reindeer) to
convert Cargo dependencies into Buck2 build rules.

### Steps to enable:

1. Create `third-party/rust/Cargo.toml`:
   ```toml
   [package]
   name = "third-party"
   version = "0.0.0"
   publish = false

   [dependencies]
   clap = { version = "4", features = ["derive"] }
   ```

2. Install and run reindeer:
   ```bash
   cargo install --locked --git https://github.com/facebookincubator/reindeer
   reindeer --third-party-dir third-party/rust vendor
   reindeer --third-party-dir third-party/rust buckify
   ```

3. Update BUCK file to uncomment the deps.

## Future: Turnkey Integration

A future turnkey feature could automate this with:
- A `rust-deps.toml` similar to `go-deps.toml`
- A `rustdeps-gen` tool similar to `godeps-gen`
- Auto-generation of third-party BUCK files
