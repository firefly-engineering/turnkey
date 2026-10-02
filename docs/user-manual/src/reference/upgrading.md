# Upgrading

What changes when a project moves to a newer turnkey, and what to do about it.

## The per-distribution Python cell

The Python dependency cell is now built one package per distribution, and
laid out by `tk materialize` as a real directory, `.turnkey/pydeps`, instead
of one symlink to a merged cell
([ADR 0004](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0004-deps-cells-are-write-once-directories.md),
[ADR 0010](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0010-pydeps-stores-one-distribution-per-store-link.md)).
Labels (`pydeps//vendor/<name>:<name>`) are unchanged. A distribution bump
now rebuilds that distribution alone, plus those whose dependencies name it
if theirs changed, and re-runs only its dependents' actions, with no daemon
restart.

### Switching over

- **The first shell load** after upgrading replaces the `.turnkey/pydeps`
  symlink with the directory. `tk` restarts the buck2 daemon once, and the
  next build is a full one. Nothing else needs doing.
- **A lock holding several versions of one distribution fails.** `pydeps-gen`
  now rejects a uv resolution that forks on a marker, since the cell holds one
  version per name: pin the dependency so the lock no longer forks (see
  [Python Workspaces](../workflows/python-workspace.md#one-version-per-distribution)).
- **User patches move.** They go under
  `.turnkey/patches/pydeps/vendor/<name>/`, one directory per distribution,
  and apply in that distribution's own derivation. A patch file directly
  under `.turnkey/patches/pydeps/` fails evaluation with where to move it:
  move it into the directory of the distribution whose files it changes,
  keeping its content. A patch spanning two distributions is split into one
  per distribution.

### Rolling back

```bash
rm -rf .turnkey/pydeps .turnkey/pydeps.lock .turnkey/gcroots/pydeps
```

## The JavaScript cell's package graph

The JavaScript dependency cell now holds each locked package once, at
`vendor/<name>@<version>`, and one target per pnpm snapshot, laid out as
pnpm lays out `node_modules/.pnpm`
([ADR 0012](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md)).
A package's own dependencies resolve without the consumer declaring them,
two versions of one name and peer resolutions coexist, and dependency
cycles build. `tk materialize` lays the cell out as a real directory,
`.turnkey/jsdeps`, so a package bump re-runs only what depends on it, with
no daemon restart, and plain `buck2` reads the new version.

### Switching over

- **Labels are npm names.** `jsdeps//:types_lodash` is now
  `jsdeps//:@types/lodash`. Rules sync rewrites the labels it manages;
  change those in a `turnkey:preserve` section, or in a target sync doesn't
  manage, by hand.
- **Only direct dependencies have labels**: the root `package.json`'s
  (`js-deps.toml`'s `[direct]`). A target that listed a package only some
  dependency needs drops it. `@types/...` packages are usually
  `devDependencies`: set `buck2.javascript.includeDevDependencies = true`.
- **Regenerate `js-deps.toml`** with `tk sync`: the cell needs its
  `[[instance]]` and `[direct]` tables, which need a pnpm 9 lockfile.
- **The first shell load** replaces the `.turnkey/jsdeps` symlink with the
  directory. `tk` restarts the buck2 daemon once, and the next build is a
  full one.
- **User patches move** under
  `.turnkey/patches/jsdeps/vendor/<name>@<version>/`. A patch file directly
  under `.turnkey/patches/jsdeps/` fails evaluation. See
  [Dependency Fixups](../workflows/fixups.md).

### Rolling back

```bash
rm -rf .turnkey/jsdeps .turnkey/jsdeps.lock .turnkey/gcroots/jsdeps
```

## The per-package Solidity cell

The Solidity dependency cell is now built one package per Solidity package,
and laid out by `tk materialize` as a real directory, `.turnkey/soldeps`,
instead of one symlink to a merged cell
([ADR 0004](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0004-deps-cells-are-write-once-directories.md),
[ADR 0011](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0011-soldeps-stores-one-package-per-store-link.md)).
Labels (`soldeps//:<package>`, `soldeps//:bundle`), the root
`remappings.txt` and native `forge` are unchanged. A package bump no longer
restarts the buck2 daemon: it re-runs the Solidity actions, and nothing in
another language. Plain `buck2` reads the new version.

### Switching over

- **The first shell load** after upgrading replaces the `.turnkey/soldeps`
  symlink with the directory. `tk` restarts the buck2 daemon once, and the
  next build is a full one. Nothing else needs doing.
- **A name declared twice** (in `foundry.toml` and `package.json`, or in
  both `dependencies` and `devDependencies`) must resolve to one package:
  `tk sync` writes it once, or fails naming each declaration when they
  differ. The cell holds one version per name.
- **User patches move.** They go under
  `.turnkey/patches/soldeps/vendor/<name>/`, one directory per package, and
  apply in that package's own derivation. A patch file directly under
  `.turnkey/patches/soldeps/` fails evaluation with where to move it. See
  [Dependency Fixups](../workflows/fixups.md).

### Rolling back

```bash
rm -rf .turnkey/soldeps .turnkey/soldeps.lock .turnkey/gcroots/soldeps
```

## The per-module Go cell

The Go dependency cell is now built one package per module, and laid out by
`tk materialize` as a real directory, `.turnkey/godeps`, instead of one
symlink to a merged cell
([ADR 0004](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0004-deps-cells-are-write-once-directories.md),
[ADR 0008](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0008-godeps-stores-one-module-per-store-link.md)).
Labels (`godeps//vendor/<import path>:<last component>`) are unchanged. A
module bump now rebuilds that module alone and recompiles only its
dependents: 8 actions for a one-module bump in turnkey's own repo.

### Switching over

- **The first shell load** after upgrading replaces the `.turnkey/godeps`
  symlink with the directory. `tk` restarts the buck2 daemon once, and the
  next build is a full one. Nothing else needs doing.
- **User patches move.** They go under
  `.turnkey/patches/godeps/vendor/<module path>/`, one directory per
  module, and apply in that module's own derivation. A patch file directly
  under `.turnkey/patches/godeps/` fails evaluation with where to move it:
  move it into the directory of the module whose files it changes, keeping
  its content. A patch spanning two modules is split into one per module.

### Rolling back

```bash
rm -rf .turnkey/godeps .turnkey/godeps.lock .turnkey/gcroots/godeps
```

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
