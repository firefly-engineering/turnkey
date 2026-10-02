# TypeScript Support

Turnkey provides TypeScript support via custom Buck2 rules.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
nodejs = {}
typescript = {}
```

## Project Structure

```
my-project/
└── ts/
    └── myapp/
        ├── src/
        │   └── index.ts
        ├── tsconfig.json     # Optional
        └── rules.star
```

## Build Rules

In `rules.star`:

```python
load("@prelude//typescript:typescript.bzl", "typescript_binary", "typescript_library")

typescript_library(
    name = "lib",
    srcs = glob(["src/**/*.ts"]),
)

typescript_binary(
    name = "myapp",
    main = "src/index.ts",
    srcs = glob(["src/**/*.ts"]),
    deps = [":lib"],
)
```

## Running TypeScript

```bash
tk run //ts/myapp:myapp
```

A `typescript_test` takes the same attributes as a `typescript_binary`. It
compiles the code and runs it with node: the test passes when it exits 0.

```python
typescript_test(
    name = "myapp_test",
    main = "src/index.test.ts",
    srcs = glob(["src/**/*.ts"]),
)
```

```bash
tk test //ts/myapp:myapp_test
```

## Configuration

The TypeScript toolchain uses sensible defaults. For custom configuration, provide a `tsconfig.json`:

```python
typescript_binary(
    name = "myapp",
    main = "src/index.ts",
    srcs = glob(["src/**/*.ts"]),
    tsconfig = "tsconfig.json",
)
```

## npm Dependencies

npm packages are declared in the root `package.json` and locked in
`pnpm-lock.yaml`. `tk sync` runs `jsdeps-gen`, which writes
`js-deps.toml`, and the `jsdeps` cell is built from it
([ADR 0012](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md)).
Rules sync writes a target's `npm_deps` from the packages its sources
import.

### Labels

A target lists the packages it imports in its `npm_deps`, by their npm
name, verbatim:

```python
typescript_binary(
    name = "app",
    main = "main.ts",
    srcs = ["main.ts"],
    npm_deps = [
        "jsdeps//:lodash",
        "jsdeps//:@types/lodash",
    ],
)
```

Only the root `package.json`'s dependencies (`js-deps.toml`'s `[direct]`)
have such a label, at the version the root resolves them to. Whatever they
depend on is in the cell too, but code can't import it, just as pnpm keeps
it out of the root `node_modules`: declare a package in `package.json` to
import it. `@types/...` packages are usually `devDependencies`: set
`buck2.javascript.includeDevDependencies = true` so they are direct too.

Labels used to name a scoped package with the `@` dropped and `/` as `_`
(`jsdeps//:types_lodash`). Rules sync replaces them with the npm names
([Upgrading](../reference/upgrading.md)).

### The `node_modules` each target sees

Each locked `name@version` is fetched once, as its own store path. Each
*instance* of it, one per pnpm snapshot, is a target in the cell: a package
that pnpm installs once per peer resolution (`react-dom` with React 17 and
with React 18) has one instance per resolution, and two versions of one
name are two instances.

An instance's output is a real `node_modules/<name>/` directory, copied
from the package's files, with each of its dependencies a relative symlink
beside it, into that dependency's instance. A target's `node_modules` links
each of its `npm_deps` to its instance's package directory. node and tsc
find a package's own dependencies by walking `node_modules` up from its
real path, as in pnpm's `node_modules/.pnpm`, so each package sees exactly
the dependencies its lock entry resolves, with no `--preserve-symlinks` and
no `NODE_PATH`. A `typescript_binary`'s output has its own `node_modules`
link beside the compiled code, so `node dist/main.js` resolves the same
way, for CommonJS (`.cts`, compiled to `.cjs`) and ES modules (`.mts`, to
`.mjs`) alike.

Instances that depend on each other in a cycle are laid out together, as
one target with a forwarding target per instance.

A bump of one package rebuilds its store path and re-runs only the
instances that depend on it, and their consumers: `.turnkey/jsdeps` is a
real directory that `tk materialize` keeps in line with the cell index, so
plain `buck2` reads the new version without a daemon restart.

Copying costs disk: every installed package is copied once per
configuration under `buck-out`.

### `js-deps.toml`

One `[[package]]` per locked `name@version` (its contents), one
`[[instance]]` per pnpm snapshot (its dependencies, by the name it imports
each as, resolved to instance keys), and `[direct]`:

```toml
[[package]]
name = "chokidar"
version = "3.6.0"
url = "https://registry.npmjs.org/chokidar/-/chokidar-3.6.0.tgz"
integrity = "sha512-..."

[[package]]
name = "fsevents"
version = "2.3.3"
url = "https://registry.npmjs.org/fsevents/-/fsevents-2.3.3.tgz"
integrity = "sha512-..."
os = ["darwin"]

[[instance]]
key = "chokidar@3.6.0"
name = "chokidar"
version = "3.6.0"

[instance.dependencies]
anymatch = "anymatch@3.1.3"
braces = "braces@3.0.3"

[instance.optional_dependencies]
fsevents = "fsevents@2.3.3"

[direct]
chokidar = "chokidar@3.6.0"
```

- An instance's `key` is pnpm's snapshot key verbatim, peer groups
  included (`react-dom@18.2.0(react@18.2.0)`). Its target in the cell is
  named as pnpm names its directory in `node_modules/.pnpm`
  (`react-dom@18.2.0_react@18.2.0`).
- `os`, `cpu` and `libc` are the package's own restrictions, as npm names
  them (`darwin`, `x64`, `glibc`, or `!win32` to exclude one). A field
  that's absent allows anything.
- `link:` and `file:` dependencies fail `jsdeps-gen`, naming the package.

### Packages from a registry Nix can't reach

A package locked from a registry that Nix can't fetch from, such as a
local one, can be kept in the project as its tarball, keyed by the URL
`pnpm-lock.yaml` records for it. The cell reads the file instead of
fetching, and still checks it against the lock's integrity:

```nix
buck2.javascript.tarballs = {
  "http://localhost:4873/-/acme-utils-1.0.0.tgz" = ./registry/acme-utils-1.0.0.tgz;
};
```

### Platform-specific packages

An instance whose optional dependencies install on some platforms only,
such as esbuild's per-platform binaries or chokidar's `fsevents`, is
resolved in the cell, for every platform in `buck2.platforms`: the
optional dependencies the platform gets (their `os` and `cpu` allow it,
and on Linux their `libc` allows glibc) are linked beside it, as a
`select()` keyed like rules sync's, with no `DEFAULT`. A consumer lists the
package unconditionally. `src/examples/typescript-platform-deps` is an
example.

### Patching a package

User patches go under `.turnkey/patches/jsdeps/vendor/<name>@<version>/`,
or `vendor/<name>/` for the version a direct dependency resolves to, and
apply to that package's files with `--fuzz=0`
([Fixups](../workflows/fixups.md)).
