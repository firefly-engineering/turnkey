---
status: accepted
---

# Rust dependency resolution comes from `cargo metadata`

The features each vendored crate is built with, and the exact version each of its dependencies resolves to, come from **`cargo metadata`**, run by `tk sync`. They do not come from turnkey's own resolver. `rustdeps-gen` runs `cargo metadata --format-version 1 --locked --filter-platform <triple>` once per configured platform and merges the results. It records each crate's **package slice** in `rust-deps.toml`: its features as one flat list, and its normal dependencies as `name@version` with their rename and the platforms each applies on.

Each crate's own derivation then generates its `rules.star` from its slice alone. That is what lets a crate's store path change only when the crate's own inputs change ([ADR 0004](0004-deps-cells-are-write-once-directories.md)).

The hand-written resolver existed because resolution ran inside the cell's Nix derivation, where no `cargo` was available. That resolver is `features.py`'s unifier, `semver.py` and `gen-rust-buck`'s version matching. Once resolution moves to `tk sync` time, `cargo` is in the shell of any Rust project. The resolver was already judged by how closely it matched `cargo tree`: 229 of 230 crates in [#57](https://github.com/firefly-engineering/turnkey/issues/57)'s fix. So cargo becomes the source of truth instead of a reference we chase.

Decided in [Do we keep one rustdeps cell derivation, and which generation fixes do we take?](https://github.com/firefly-engineering/turnkey/issues/95), part of [Rust dependency cell: correct feature resolution and cheaper BUCK generation](https://github.com/firefly-engineering/turnkey/issues/82).

## Considered options

- **Port turnkey's resolver into `rustdeps-gen`** (Rust), reading the crates.io sparse index. Rejected. It would be a third implementation of Cargo's rules, after Python and Go, and every one of them is a second source of truth that can drift from Cargo.
- **Run the existing Go resolver (`cargofeatures`) at sync time.** Rejected for the same reason, although it would reuse code that shares test vectors with the Python one.
- **A single unfiltered `cargo metadata` run.** Rejected. It resolves for every target, Windows included, and brings in features and edges no configured platform builds. Running once per platform keeps today's semantics, and gives each edge its platforms directly.

## Consequences

- **`rust-features.toml` is retired.** It holds only overrides, and they are dropped. Cargo takes no "add these features" or "never enable this" input. Applying overrides on top of its result would need turnkey's resolver again. Features are asked for in `Cargo.toml`, and cargo resolves them. None were in use. A real need to never enable a feature would be designed as a fixup field.
- **The hand-written resolver is retired** once `rules.star` is byte-identical through the new path, apart from documented differences. The only known one is `tokio`'s Windows-only feature, which now follows cargo. The Go `cargofeatures` and `cargocfg` stay, because rules sync uses them.
- **The regression test becomes a comparison with `cargo tree`,** instead of hand-written fixtures of Cargo's rules.
- **`tk sync` needs `cargo` and the registry.** `cargo metadata` downloads every package into `~/.cargo/registry` on first use, then works offline from that cache. `--locked` keeps sync from ever rewriting `Cargo.lock`.
- **More files make the Rust sync rule stale:** every workspace member's `Cargo.toml` joins the root `Cargo.toml` and `Cargo.lock`. A features-only edit in a member now changes the slices.
