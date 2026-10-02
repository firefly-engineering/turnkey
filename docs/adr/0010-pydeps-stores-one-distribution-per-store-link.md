---
status: accepted
---

# The pydeps cell stores one distribution per store link, at one version per name

The `pydeps` cell follows [ADR 0004](0004-deps-cells-are-write-once-directories.md): write-once store links and alias packages, kept in line with a cell index by the materializer. In Python, the **locked package** is a distribution, and it is also one buck2 package. So the Python cell is the Rust cell's shape with Go's single version:

- **One derivation, and one store link, per distribution** at its locked version. Fixups already apply there. The store link holds the distribution's locked wheel, not its sdist ([ADR 0013](0013-pydeps-distributions-are-their-locked-wheels.md)).
- **Each distribution's derivation writes its own `rules.star`** with `pydeps-cell`, from nothing but its **package slice**, the platforms' conditions and the Python toolchain's version. The slice's dependencies are already narrowed to those the cell holds when Nix evaluates, so no derivation reads the rest of the cell. Today's merge step, which rewrites every package's `rules.star` over the whole cell, goes away.
- **Labels stay `pydeps//vendor/<name>:<name>`.** The materializer writes one alias package per distribution, at `vendor/<name>`, forwarding to `//_store/<store basename>:<name>`. A `rules.star` names its dependencies the same way, relative to the cell, so they resolve through the alias packages.
- **One version per name, and only unversioned alias packages.** `pydeps-gen` fails when the lock holds several versions of one name (a uv forked resolution), and names the package and its markers.
- **The cell index is the Rust shape without version aliases:** one package per distribution, its store path and its targets.
- **User patches are per distribution**, under `<patches>/pydeps/vendor/<name>/*.patch`, applied in the distribution's derivation before its `rules.star` is written. A flat patch file under `<patches>/pydeps/` fails evaluation and names the new path.

The result is that a `python-deps.toml` change rebuilds only the distributions whose slice changed, and re-runs only their reverse-dependency closure, with no daemon restart.

Decided in [How does a write-once pydeps cell map Python distributions onto store links?](https://github.com/firefly-engineering/turnkey/issues/235), part of [Write-once deps cells for the remaining languages](https://github.com/firefly-engineering/turnkey/issues/234).

## Considered options

- **Keep `pydeps-cell` as a merge step over the whole cell.** Rejected. Every change would rewrite every package's `rules.star`, which is the cost the write-once cell exists to remove.
- **Several versions per name, as in Rust:** packages keyed `name@version`, version alias packages, and each `rules.star` choosing a version per platform with `select()`. Rejected for now. It reaches into rules sync, and turnkey builds for one Python version, so most `python_version` forks come down to one entry anyway.
- **Keep both forks in `python-deps.toml` and pick in the cell**, by evaluating their markers for the toolchain's Python and platforms. Deferred. It is the cheap way out if forks turn out to be common, and it still fails when two of turnkey's platforms need different versions.
- **Keep the last of several versions, as `pydeps-gen` did.** Rejected. It silently builds against a version that can be wrong for the toolchain's Python.

## Consequences

- **`pydeps-gen` rejects some locks that uv accepts.** A project whose lock forks on a marker has to pin the dependency until forks are supported.
- **Changing the platforms or the Python toolchain's version rebuilds every distribution.** Each one's `rules.star` really can change, because markers are evaluated for both.
- **A workspace member that starts asking a distribution for an extra changes that distribution's store path.** Its dependencies really do change.
- **Flat pydeps patches stop working.** `tk compose patch` already writes one patch per package for a materialized cell.
