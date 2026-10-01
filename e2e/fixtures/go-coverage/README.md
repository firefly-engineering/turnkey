# go-coverage fixture

A small `go.work` monorepo that exercises every feature of turnkey's Go path
that turnkey's own Go code used to be the only test of. The list comes from
[What Go coverage must examples and fixtures have before turnkey's own Go code goes away?](https://github.com/firefly-engineering/turnkey/issues/192),
and the fixture is [#208](https://github.com/firefly-engineering/turnkey/issues/208).
The `go-coverage` e2e test (`e2e/tests/10-go-coverage.sh`) runs it through the
whole path: `tk sync` → `godeps` cell → rules sync → `buck2 build`/`test`.

```bash
./e2e/harness/runner.sh go-coverage
```

## Layout

```
go.work                  use ./app ./lib
lib/                     module example.com/lib (no third-party deps)
  text/                  library
  greet/                 library importing text; go_test with embed_srcs, resources and an external test package
app/                     module example.com/app
  config/                BurntSushi/toml
  cache/                 golang-lru/v2 (lru and simplelru); go_test
  dump/                  go-spew, at a pseudo-version
  cmd/hello/             go_binary importing config, cache, dump and lib's greet
```

The root `flake.nix`, `toolchain.toml` and `rules.star` come from
`templates/default`; the test adds the fixture on top.

## What each feature is covered by

| Feature | Where |
| --- | --- |
| Two workspace members (ADR 0007) | `go.work` |
| Imports within a member | `lib/greet` → `lib/text`; `app/cmd/hello` → `app/config`, `app/cache`, `app/dump` |
| Imports across members | `app/cmd/hello` → `lib/greet` |
| A `/v2` module | `github.com/hashicorp/golang-lru/v2` |
| A pseudo-version | `github.com/davecgh/go-spew v1.1.2-0.20180830191138-d8f796af33cc` |
| `internal/` packages and package-to-package imports in a third-party module | golang-lru/v2 (`lru` → `simplelru` → `internal`) and BurntSushi/toml (`toml` → `internal`) |
| Upper-case letters in a module path (`!` escaping in the module cache) | `github.com/BurntSushi/toml` |
| `go_test` with `target_under_test` | `lib/greet:greet_test`, `app/cache:cache_test` |
| `embed_srcs` and `//go:embed` | `lib/greet/greet_test.go` embeds `testdata/cases.txt` |
| testdata as `resources` | `lib/greet/greet_test.go` reads `testdata/golden.txt` next to the test binary |
| An external `_test` package | `lib/greet/greet_external_test.go` (`package greet_test`) |
| Test result caching | the test runs `tk test //...` twice; the second run must reuse every result |
| A direct import also reachable through a dep's own deps ([#201](https://github.com/firefly-engineering/turnkey/issues/201)) | `app/cache` imports `simplelru` directly and through `lru`: rules sync must declare it |

## Why the deps are empty

Every target ships with `deps = []`. The test checks that `tk rules check`
reports them stale, runs `tk rules sync`, and then checks the exact labels it
wrote. Committed, synced deps would let a rules sync that misses an import go
unnoticed, as long as the build still found the package through another dep.

## Keeping it small

The third-party modules are small, pure Go, and have no dependencies of their
own. `app/go.mod` doesn't require `example.com/lib`: the workspace provides
it. `go mod tidy` ignores `go.work`, so it can't resolve that import and
fails on `app`; `go build` and `go test` work. To change a
third-party version, run `go get <module>@<version>` in the member's
directory and drop the `// indirect` marker `go get` adds.
