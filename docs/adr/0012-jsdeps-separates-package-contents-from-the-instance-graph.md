---
status: accepted
---

# The jsdeps cell separates package contents from the package instance graph

npm keeps several versions of one name, and installs one version once per peer resolution. Node and tsc find a package's dependencies by walking `node_modules` up from the package's *realpath*. So the "one version per name" that Go, Python ([ADR 0010](0010-pydeps-stores-one-distribution-per-store-link.md)) and Solidity ([ADR 0011](0011-soldeps-stores-one-package-per-store-link.md)) chose would reject almost every real lockfile. A store link to a package can't serve as its directory in `node_modules` either: its dependencies have to sit next to it. The `jsdeps` cell still follows [ADR 0004](0004-deps-cells-are-write-once-directories.md), but it splits what a package is from where it sits in the graph:

- **Contents:** one derivation, and one store link, per locked package `name@version`. Its inputs are only the tarball, its fixups and user patches. Its `rules.star` exposes the files as a contents target (`vendor/<name>@<version>:files`). That target is the content-keyed input of every instance of the package, and is never used as a package directory itself.
- **The graph:** each pnpm snapshot is a **package instance**. jsdeps-gen records the snapshots as they are and never re-resolves. `js-deps.toml` holds `[[package]]` records (contents), `[[instance]]` records (the snapshot key, and its `dependencies` and `optional_dependencies` as maps from the name the package imports to an instance key), and `[direct]` (bare name to the instance the root `package.json` resolves to). The cell's root package ([ADR 0011](0011-soldeps-stores-one-package-per-store-link.md)'s `root`) declares one `npm_instance` target per instance. The rule lives in turnkey's prelude extensions, and an instance's platform-only optional dependencies are a `select()`.
- **The layout:** an instance's output is a real directory, `node_modules/<name>/`, made by a run action that copies the contents and dereferences every link. Each of its dependencies is a relative symlink beside it, pointing into the dependency instance's output. Instance outputs opt out of content-based paths, and pass their dependency links with `ignore_artifacts`, so a link is keyed by its path. A consumer links its direct instances into its own `node_modules` and carries every instance in its closure as hidden inputs. Nothing runs with `--preserve-symlinks` or `NODE_PATH`.
- **Cycles:** each strongly connected component of instances is one target, holding all its members' directories, with one forwarding target per member. The target graph stays acyclic.
- **Labels:** an instance target is named after pnpm's own directory name for it (`react-dom@18.2.0_react@18.2.0`), hashed when it would pass the 255-byte path-component limit. Only direct dependencies get an unversioned label, and it is the verbatim npm name (`jsdeps//:@types/lodash`).
- **`link:` and `file:` dependencies** fail jsdeps-gen, naming the package.
- **User patches are per locked package**, under `<patches>/jsdeps/vendor/<name>@<version>/*.patch`, or the bare name for the version a direct dependency resolves to. A flat patch file fails evaluation.

The result: a bump adds one store link and re-runs only the bumped package's reverse-dependency closure, with no daemon restart. Measured in [Prototype the jsdeps virtual store end to end](https://github.com/firefly-engineering/turnkey/issues/245) with tsc 6 and 7 and node (CJS and ESM): a one-leaf bump re-ran 7 actions against 24 cold. That prototype covered transitive dependencies, two versions of one name, a peer split, a cycle and transitive `@types`.

Decided in [What is a jsdeps package: versions, peers and the node_modules each consumer sees?](https://github.com/firefly-engineering/turnkey/issues/243), part of [Write-once deps cells for the remaining languages](https://github.com/firefly-engineering/turnkey/issues/234).

## Considered options

- **The Rust model: edges in each store link's `rules.star`.** Rejected. A package's derivation would depend on its peers and its dependencies' versions, so contents would be duplicated across peer instances, and a dependency bump would rebuild every dependent's store path.
- **Store links as `node_modules` entries, with `--preserve-symlinks` for Node and `preserveSymlinks` for tsc.** Rejected. It needs no copies, but every tool that resolves modules has to be configured, and native modules linked from two places load twice. The prototype's first attempt, which copied the contents target as-is, failed in tsc because of realpath resolution.
- **buck2's `copied_dir` for the package directory.** Rejected. It keeps absolute links into `/nix/store` as links.
- **Content-based output paths for instances**, buck2's default. Rejected. Each dependency link would embed its target's content hash, so a content-only change re-runs every instance up the chain (10 actions against 6 in the prototype).
- **"ref" targets with the closure assembled per consumer** (rules_js's way of breaking cycles). Rejected in favour of collapsing components once, at generation time. Real locks have few, small cycles: 5, of size 2 or 3, in a 1,227-snapshot lock.
- **The highest version as the unversioned label**, as in Rust. Rejected. It could hand first-party code a version the root `package.json` never asked for.

## Consequences

- **Instance outputs cost a copy of every installed package per configuration.** About 0.8–1 GB and 11 s single-threaded for a 1,119-instance lock. The cost follows the number of files. On macOS, `/nix` is a separate volume, so neither hardlinks nor clones would help.
- **Bundling or deploying a JS target copies the whole instance closure**, never one package directory: the dependency links point outside it.
- **Labels change once:** `jsdeps//:types_lodash` becomes `jsdeps//:@types/lodash`. Rules sync rewrites them.
- **Remote execution of instance actions is unverified.** Uploading trees with escaping relative links worked in the prototype, but no action-cache hit was exercised.
- **A fixture with a peer split and a cycle needs a local registry**, because `file:` dependencies are rejected.
