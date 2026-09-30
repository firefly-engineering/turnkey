---
status: accepted
---

# Go modules share one resolution through `go.work`

A turnkey repository has **one Go resolution and one `godeps` cell**, however many Go modules it holds. Several modules are supported only as a **`go.work` workspace** at the project root. Go computes a single build list (MVS) across every workspace member, so each module path has exactly one version in the repo. This is the same shape as a Cargo workspace (one `Cargo.lock`) and a uv workspace (one `uv.lock`), each of which already feeds one deps cell.

The build list comes from Go, not from turnkey. `godeps-gen` runs `go list -m -json all` in workspace mode for the versions. It takes the set of modules from the members' tidy `go.mod` requires, minus the members themselves, so the set does not depend on the platform `tk sync` runs on. That follows [ADR 0005](0005-rust-resolution-comes-from-cargo-metadata.md): the language's own tool resolves, and turnkey does not keep a second implementation of its rules.

A repo without a `go.work` is a workspace of one member, the root `go.mod`, and behaves as before.

Decided in [Does a turnkey repo need more than one Go module, and is each one a cell?](https://github.com/firefly-engineering/turnkey/issues/145), part of [Go dependency cell: ready for a real monorepo](https://github.com/firefly-engineering/turnkey/issues/81).

## Considered options

- **One `godeps` cell per Go module**, with each BUCK target naming the cell it uses. This is the design in [Support multiple godeps cells per repository](https://github.com/firefly-engineering/turnkey/issues/41). Rejected. Its purpose is to let isolated modules pin conflicting versions. Two versions of one module path in one Buck2 graph invite diamond conflicts wherever the graphs meet. They also defeat the single update point that the one-resolution rule exists to give. Every layer that now assumes one cell per language would have to change: the language record, the `turnkey.buck2.go` options, the rules-sync mapper and `.buckconfig`. So would every third-party label.
- **Sibling modules joined only by local `replace` directives, with no `go.work`.** Rejected. Each module then resolves on its own, with its own `go.sum`, so the same dependency can land at different versions. That is the multi-cell case without the cells. A local `replace` must now point at a workspace member.
- **Union the members' `go.mod` requires, and require `go work sync`.** Rejected. It is cheap and offline, but it approximates MVS. Workspace MVS can select a version higher than any single member's `go.mod` names, when a dependency's own `go.mod` asks for it.
- **Take the module set from `go list -deps ./...`.** Rejected. Its answer depends on the GOOS, GOARCH and build tags of the machine running `tk sync`, and would drop modules that only other configured platforms need.

## Consequences

- **`tk sync` needs `go` and the module proxy** to resolve a workspace. `go list -m all` downloads the `.mod` files of the pruned graph into the module cache. After that it works from the cache.
- **More files make the Go sync rule stale:** `go.work`, `go.work.sum`, and each member's `go.mod` and `go.sum`. `go-deps.toml` records them, as `rust-deps.toml` records its manifests. The `tw` `go` wrapper watches the same files, and treats `go work` as mutating.
- **Members are the only first-party Go modules.** Rules sync maps an import to the member whose module path is its longest prefix on a `/` boundary. Imports of a member's module path from inside the `godeps` cell, such as a patched local fork of a third-party module, map to that member's targets. A `go.mod` that is not a member is outside the Go build, as it is for `go ./...`: test fixtures need no fencing.
- **Turnkey's own repository keeps one root `go.mod`.** That rule is a choice for this repo, not a limit of the framework.
