---
status: accepted
---

# The soldeps cell stores one package per store link, and a deps cell's root package comes from its index

The `soldeps` cell follows [ADR 0004](0004-deps-cells-are-write-once-directories.md): write-once store links and alias packages, kept in line with a cell index by the materializer. In Solidity, the **locked package** is an npm package or a git dependency, and it is also one buck2 package. So the mapping is the one [ADR 0010](0010-pydeps-stores-one-distribution-per-store-link.md) gives Python:

- **One derivation, and one store link, per package** at its locked version (npm) or revision (git). The two sources differ only in how the package is fetched.
- **One version per name, and only unversioned alias packages**, at `vendor/<name>`. `soldeps-gen` collapses a package declared twice with the same pin, and fails when the pins differ.
- **User patches are per package**, under `<patches>/soldeps/vendor/<name>/*.patch`, applied in the package's derivation. A flat patch file under `<patches>/soldeps/` fails evaluation and names the new path.
- **Labels stay as they are:** `soldeps//:<target>` for a package and `soldeps//:bundle` for all of them. The bundle keeps its `vendor/<name>` output paths, so the root `remappings.txt` doesn't change.

Solidity is the first cell whose users address its **root package**, so the root needs a home in a write-once cell:

- **The cell index carries the root package's build file as text** (`root`). Nix generates it from the package list. The materializer writes it as it writes the cell's `.buckconfig`, without parsing it, and it names packages through their alias packages (`//vendor/<name>:<target>`). It is a real file rewritten when the package list changes, as ADR 0004 allows for everything that changes.

A bump no longer needs a daemon restart, and nothing reads the cell stale. It still re-runs **every Solidity action**, because each one stages the whole bundle as one input. Staging only the packages a target depends on is a question about the rules, not about the cell: [Solidity actions stage only the soldeps packages they depend on](https://github.com/firefly-engineering/turnkey/issues/241).

Decided in [How does a write-once soldeps cell map Solidity packages onto store links, given every action reads the whole bundle?](https://github.com/firefly-engineering/turnkey/issues/237), part of [Write-once deps cells for the remaining languages](https://github.com/firefly-engineering/turnkey/issues/234).

## Considered options

- **No root package:** labels become `soldeps//vendor/<name>:<target>`, and the bundle moves into a `vendor` package. Rejected. It changes the rules sync Solidity mapper, the rules' bundle default and every user build file, and `jsdeps` still needs a root for its per-platform `npm_package` targets.
- **The root package as a store link.** Rejected. Its contents change with the package list, so it would be a link retargeted on every bump, which ADR 0004 rules out.
- **Holding the layout back until actions stage only their own packages.** Rejected. The layout alone already removes stale reads, and it turns a full rebuild in every language into a rebuild of the Solidity actions.

## Consequences

- **The cell index gains `root`.** Rust and Go leave it empty, and their cells keep no root build file.
- **A Solidity package bump re-runs every Solidity action** until the narrowing question is settled.
- **Flat soldeps patches stop working.** `tk compose patch` already writes one patch per package for a materialized cell.
