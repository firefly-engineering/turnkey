# Upgrading

What changes when a project moves to a newer turnkey, and what to do about it.

## The per-crate Rust cell

The Rust dependency cell is now built one package per crate, and laid out by
`tk materialize` as a real directory, `.turnkey/rustdeps`, instead of one
symlink to a merged cell
([ADR 0004](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0004-deps-cells-are-write-once-directories.md)).
Each crate's features and dependencies come from cargo itself
([ADR 0005](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0005-rust-resolution-comes-from-cargo-metadata.md),
[ADR 0006](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0006-package-slices-follow-cargos-feature-resolver.md)).
A dependency bump now recompiles only the changed crates' dependents and
re-runs only their tests: about 90 actions for a one-crate bump in turnkey's
own repo, against about 1,200 before.

### Switching over

- **The first shell load** after upgrading regenerates `rust-deps.toml`
  (schema 2, with each crate's package slice) and replaces the
  `.turnkey/rustdeps` symlink with the directory. `tk` restarts the buck2
  daemon once, and the next build is a full one. Nothing else needs doing.
- **`tk sync` now runs cargo** (`cargo tree` and `cargo metadata`, with
  `--locked`): `Cargo.lock` must match your `Cargo.toml` files, and the first
  run downloads the crates into `~/.cargo/registry`.
- **Every workspace member's `Cargo.toml` is a source of `rust-deps.toml`,**
  so a features-only edit in a member regenerates it.

### Rolling back

To go back to an older turnkey, remove the directory before reloading the
shell, so that the older shell can create its symlink again:

```bash
rm -rf .turnkey/rustdeps .turnkey/rustdeps.lock .turnkey/gcroots/rustdeps
```

### `rust-features.toml` is retired

`buck2.rust.featuresFile` no longer exists, and setting it fails evaluation
with a message pointing here. Its overrides are dropped: each crate gets the
features cargo resolves for your workspace.

- **To get a feature,** ask for it in the `Cargo.toml` of the member that uses
  the crate, for example `serde = { version = "1", features = ["derive"] }`.
- **Then** remove the option and delete `rust-features.toml`.

`[[requested]]` is gone from `rust-deps.toml` too; regenerating it removes it.

### Features may differ slightly

The old resolver was turnkey's own; the new one is cargo's. In turnkey's own
repo, four of 317 crates changed, all to what `cargo build` does:

- a crate no longer gets dependencies that only apply on targets you don't
  build (`jiff`'s `portable-atomic`);
- features that only one platform enables are set on that platform only
  (`mio`'s `log`, `zerocopy`'s `derive`);
- features no configured platform enables are gone (`tokio`'s `windows-sys`).

A crate that no configured platform builds (Windows-only, wasm, a build
dependency) gets no features or dependencies, since nothing configures it.

### User patches

Patches that `tk compose patch` writes for the Rust cell now live in their
crate's directory, `.turnkey/patches/rustdeps/vendor/<crate>@<version>/`, and
apply in that crate's own package. A patch file left directly under
`.turnkey/patches/rustdeps/` fails evaluation: move it into its crate's
directory, or regenerate it with `tk compose patch`. See
[Dependency Fixups](../workflows/fixups.md).
