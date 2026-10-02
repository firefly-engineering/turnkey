# jsdeps-graph fixture

A small pnpm project whose lock has every shape of npm package graph the
`jsdeps` cell must lay out
([ADR 0012](../../../docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md),
[#232](https://github.com/firefly-engineering/turnkey/issues/232)). The
`jsdeps-graph` e2e test (`e2e/tests/11-jsdeps-graph.sh`) runs it through the
whole path: `tk sync` → `jsdeps` cell → rules check → `tk build`/`tk test`.

```bash
./e2e/harness/runner.sh jsdeps-graph
```

## Layout

```
package.json, pnpm-lock.yaml   the project's dependencies, locked by pnpm
.npmrc                         @tkfixture packages come from the static registry
registry/serve.mjs             the static registry, for regenerating the lock
registry/packages/             the @tkfixture packages' sources
registry/tarballs/             the @tkfixture packages, as pnpm pack wrote them
app/esm.mts                    ES module code: asserts in types and at runtime
app/cjs.cts                    the same, as CommonJS code
app/undeclared.ts              undeclared transitive deps don't resolve in types
app/rules.star                 a typescript_test per module system, and a
                               typescript_library
```

## What each shape is covered by

| Shape | Packages | Asserted |
| --- | --- | --- |
| Undeclared transitive deps | `micromatch` → `braces` → `fill-range` → `to-regex-range` → `is-number`, and `picomatch` 2 | `micromatch.isMatch`, `micromatch.braces` |
| Two versions of one name | `@tkfixture/host` 1.0.0 (the project's) and 2.0.0 (the wrapper's); `picomatch` 4.0.2 and 2.3.2 | the host versions' literal types and values; each `picomatch/package.json` |
| A peer split | `@tkfixture/plugin` with `@tkfixture/host` as a peer: installed once with host 1.0.0 (the project's) and once with 2.0.0 (`@tkfixture/wrapper`'s) | `hostVersion()` is `"1.0.0"`, `wrappedHostVersion()` `"2.0.0"`, in types and values |
| A cycle | `@tkfixture/cyc-a` ↔ `@tkfixture/cyc-b` | the mutually recursive `A`/`B` types; `viaB()` goes to cyc-b and back |
| Transitive `@types` | `@types/micromatch` → `@types/braces`; `@types/node` → `undici-types` | `micromatch.braces`'s options type is `@types/braces`'; tsc checks `@types/node`'s declarations, which import `undici-types` |
| An ES module package with an ES module dep | `p-limit` → `yocto-queue` | `pLimit` from both ES module and CommonJS code |
| Strictness | none of `braces`, `fill-range`, `yocto-queue`, `@tkfixture/cyc-b`, `@types/braces` is the project's | tsc fails to resolve them (`undeclared.ts`), node to import or require them |

Each test's assertions are type-checked by `tk build` (an annotation such
as `const wrapperPlugin: "2.0.0"` fails to compile if the wrong host is
seen) and run by `tk test` under node.

## Why a static registry

The cycle and the peer split need packages shaped for them, which no public
package is in a small enough graph, and the cell rejects `file:` and `link:`
dependencies. So the `@tkfixture` packages are published to a small
static registry, `registry/serve.mjs`, while locking: the lock records their
`http://localhost:4873/-/...` tarball URLs and integrity, like any registry
package's.

Nix can't fetch from that registry, which only runs while locking. The
tarballs are kept in `registry/tarballs/`, and the test's flake hands them to
the cell with `buck2.javascript.tarballs`, keyed by those URLs: the cell reads
each from the project and checks it against the lock's integrity, as a fetch
would. The public packages are fetched from npm as usual.

## Regenerating the lock

```bash
# After changing a package's sources, pack it again
(cd registry/packages/plugin && pnpm pack --pack-destination ../../tarballs)

node registry/serve.mjs &
pnpm install --lockfile-only
kill %1
```
