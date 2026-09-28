---
status: accepted
---

# Package slices follow Cargo's feature resolver, read through `cargo tree`

A crate's **package slice** comes from what Cargo compiles, not from its lock file. `rustdeps-gen` runs `cargo tree --target <triple> -e normal,dev --prefix depth --format '{p}|{f}'` once per configured platform. The edges and the features of every crate come from its output, with a crate's host and target units unioned, since Buck2 builds one target for both. `cargo metadata`, which names every real edge correctly, gives the name a dependent's code uses for each dependency. Features, like dependencies, record the platforms they apply on. This supersedes the part of [ADR 0005](0005-rust-resolution-comes-from-cargo-metadata.md) that took features and edges from `cargo metadata`. The rest of 0005 stands: cargo resolves at `tk sync` time, and turnkey's own resolver is retired.

`cargo metadata`'s graph is the lock file's resolution, and it is larger than what `cargo build` compiles. Cargo's lock file keeps every optional dependency that a **weak dependency feature** (`dep?/feat`) names, and `cargo metadata` reports those as edges, with the features they carry. The feature resolver activates them only when something else enables the dependency. On turnkey's own lock file, that gave 8 crates edges or features that `cargo build` never compiles: for example `url` → `serde`, through `std = [..., "serde?/std"]`; `rustix` → `libc` on Linux; and `serde_core/alloc`. Evidence is in [Where do Rust package slices come from, when cargo metadata over-activates weak dependency features?](https://github.com/firefly-engineering/turnkey/issues/164).

The resolution is the one `cargo test --workspace` uses: members' dev-dependencies count, and their features unify into the shared crates, because one vendored target serves both libraries and tests. Build-dependencies are left out, because Buck2 never runs build scripts.

## Considered options

- **Keep `cargo metadata` and document the extra edges and features.** Rejected: Buck2 would compile crates and features `cargo build` doesn't, and the list grows with every crate that uses weak dependency features.
- **`cargo metadata`, then drop what only a weak feature activated.** Rejected. It needs each crate's feature table and Cargo's activation rules, which is the hand-written resolver ADR 0005 retires, back in a smaller form.
- **`cargo build --unit-graph`.** Rejected: it's exact JSON, but nightly-only. On stable it would need `RUSTC_BOOTSTRAP` or Cargo's channel override, which are unsupported.
- **One flat features list, the union over platforms.** Rejected. Under Cargo's feature resolver, `mio` and `zerocopy` get different features on Linux and macOS, and a union builds macOS with Linux's.

## Consequences

- **`cargo tree` has no JSON output.** Its output is read as line-oriented text in the format that `--format` and `--prefix depth` fix. A hermetic, vendored fixture workspace pins both the parsing and Cargo's output format: a Cargo release that changes the format fails a test rather than producing wrong slices.
- **A dependency carries a `rename` whenever its dependent's code names it differently from its package name, with `-` turned into `_`.** That covers `package = ...` renames and packages whose library has its own name (`rustls-webpki`'s is `webpki`).
- **Two known differences from today's resolver, both following cargo:**
  - `jiff` loses its `portable-atomic` edges, which apply only without pointer-width atomics.
  - `mio` and `zerocopy` get their features per platform.
- **`tokio`'s `windows-sys` feature** is enabled by both today's resolver and `cargo metadata`, but on none of turnkey's platforms under the feature resolver.
