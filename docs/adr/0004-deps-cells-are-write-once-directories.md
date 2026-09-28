---
status: accepted
---

# Deps cells are real directories with write-once store links

A **deps cell** (`.turnkey/rustdeps`, and later the other languages' cells) is a real directory in the project, not a symlink to one Nix store path. It holds:

- a **store link** per package, at `_store/<store-basename>`, pointing to that package's own store path;
- an **alias package** per package path (`vendor/anyhow@1.0.100`) and per unversioned name (`vendor/anyhow`). Each is a real `rules.star` whose `alias()` targets forward to a store link's package.

A store link is named after its target, so it is only ever created or deleted, **never retargeted**. Everything that changes on a dependency bump is a real file in the project.

The reason is a buck2 behaviour. It isn't obvious, and it rules out the simpler layouts:

- **A running daemon keeps what it read through a symlink after the symlink is retargeted.** It does not invalidate build files or sources read through the old target, and `buck2 debug file-status` reports no mismatch. Measured in [Prototype per-crate rustdeps derivations end to end](https://github.com/firefly-engineering/turnkey/issues/153) and [#154](https://github.com/firefly-engineering/turnkey/issues/154).
- **buck2 keys a file behind an absolute symlink by the first symlink's target.** Under one cell symlink, every bump changes every package's key, and the whole closure recompiles: 1184 actions for a one-crate bump. Measured in [How many buck2 actions does a one-crate Cargo.lock bump re-run?](https://github.com/firefly-engineering/turnkey/issues/94).

With write-once store links, a bump recompiles only the changed package's reverse-dependency closure (87 actions against 1184). Only the tests in that closure re-run. No daemon restart is needed. Decided in [What must the local sync step own to maintain a write-once rustdeps layout?](https://github.com/firefly-engineering/turnkey/issues/155), part of [Rust dependency cell: correct feature resolution and cheaper BUCK generation](https://github.com/firefly-engineering/turnkey/issues/82).

## Considered options

- **One symlink to the whole cell, with `tk` restarting the daemon when it changes** (today's `cellfresh`). Rejected. It is correct only under `tk`. Every change becomes a full rebuild in every language, because a restart discards all in-memory state and there is no local action cache. Plain `buck2` callers get stale inputs with no error.
- **Per-package symlinks named after the package (`vendor/anyhow@1.0.100 -> /nix/store/…`), retargeted on change.** Rejected. It gives the per-package keying, but a retargeted link serves stale contents. The prototype's first bump failed on files of the removed version.
- **Per-package symlinks inside the cell's store path.** Rejected. buck2 honours only the first absolute symlink on a path, so nested links are keyed under the cell's path and every bump moves every package again.
- **Copying packages into the project.** Rejected. buck2 would key them by content, but a bump re-copies, and after a daemon restart buck2 re-hashes everything it reads. The cost of a change must follow the size of the change.

## Consequences

- **A cell index drives the cell.** The shell builds one per deps cell (the Nix-built list of package paths, store paths, target names and aliases). A `tk` subcommand, the materializer, brings the directory in line with it. The cell leaves the managed-links list, and the materializer alone owns it.
- **Nothing may retarget a store link.** The materializer only compares names, and fails loudly if an existing store link points anywhere but its name.
- **The cell must be GC-rooted explicitly.** Its store links are not roots. The materializer roots the current cell index under `.turnkey/gcroots/`.
- **Target names come from the generator that writes each `rules.star`,** carried in the cell index. The materializer never parses Starlark.
- **Switching over costs one daemon restart and one full build per checkout.** Going back to an older turnkey means removing the directory by hand.
- **Old per-hash packages accumulate in `buck-out`,** because a changed package gets a new package path, until `tk clean`.
