---
status: accepted
---

# The godeps cell stores one module per store link and aliases each Go package

The `godeps` cell follows [ADR 0004](0004-deps-cells-are-write-once-directories.md): write-once store links and alias packages, kept in line with a cell index by the materializer. In Go, the **locked package** is a module, and a module holds many Go packages. So the Go cell differs from the Rust cell in the following ways:

- **One derivation, and one store link, per module** at its locked version. Each module's derivation generates its own `rules.star` files with `buckgen`, one per Go package, from nothing but its own source and the cell-wide settings: the cell name, the platforms, the allowed build tags and the Go minor version. The `textflag.h`/`funcdata.h` assembly headers are added there too.
- **Labels stay `godeps//vendor/<import path>:<last component>`.** The materializer writes one **alias package per Go package**, at `vendor/<import path>`, forwarding to `//_store/<store basename>/<subdir>:<target>`. Alias packages nest (`vendor/cloud.google.com/go` and `vendor/cloud.google.com/go/storage`).
- **Only unversioned alias packages.** Under [ADR 0007](0007-go-modules-share-one-resolution-through-go-work.md) a module path has one version, and no label names it.
- **The Go package list comes from the module's derivation.** Its `targets` output lists each Go package `buckgen` rendered, as `<subdir> <target>`. The cell index carries one package per Go package, with a `subdir` alongside its store path. A directory no configured platform builds gets no alias.
- **Nested modules are disjoint by construction.** A module zip leaves out every subdirectory holding its own `go.mod`. When two modules in the build list still offer one import path (an old parent version that still holds a since-split directory), the index builder gives it to the longest module path. Go would reject importing such a package as ambiguous, so no building import depends on the choice.
- **Imports of a workspace member's module path from inside the cell go through a forwarding alias package.** The package list includes the import paths each module's `rules.star` files reference. For each one a member owns (the longest prefix on a `/` boundary), the index adds an alias package forwarding to `root//<member dir>/<rest>:<last component>`. Module derivations never see the members.
- **User patches are per module**, under `<patches>/godeps/vendor/<module path>/*.patch`, applied in the module's derivation before `buckgen` runs, so a patch that adds an import is seen.

The result is that a `go.mod` bump rebuilds only the modules it changes, and a `go.work` member change rewrites only alias packages.

Decided in [How does a write-once godeps cell map import paths onto per-module store links?](https://github.com/firefly-engineering/turnkey/issues/179), part of [Go dependency cell: ready for a real monorepo](https://github.com/firefly-engineering/turnkey/issues/81).

## Considered options

- **Labels that address the store link directly** (`godeps//_store/<basename>/<subdir>:<target>`). Rejected. `buckgen` would need every imported module's store path, so each module's derivation would depend on its dependencies' derivations, and one bump would rebuild everything that depends on it, transitively.
- **One derivation per Go package.** Rejected. The module is what the proxy zip holds, what `go-deps.toml` locks and what a bump changes. Splitting it adds derivations and gains nothing, because each Go package is already its own buck2 package inside the store link.
- **Passing the members to every module derivation as `buckgen`'s `local_replaces`** (today's unused code path). Rejected. It is simpler, but adding or removing any `go.work` member would rebuild every module and recompile the whole third-party closure.
- **Failing the index build when two modules offer one import path.** Rejected. It would reject repos that `go build` accepts.

## Consequences

- **The cell index gains two things:** a `subdir` on a package (empty for Rust, whose index is unchanged), and alias packages that forward to a label outside the cell, not to a store link.
- **`tk compose` resolves a path to a module and a subdirectory.** A path under a forwarding alias is first-party code and isn't composed.
- **Changing the platforms, the allowed build tags or the Go minor version rebuilds every module.** Each one's `rules.star` really does change.
- **A user patch spanning two modules is two patches.** `tk compose patch` writes one per module, as it does one per crate.
