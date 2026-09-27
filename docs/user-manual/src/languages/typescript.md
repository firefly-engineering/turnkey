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
`js-deps.toml`, and the `jsdeps` cell is built from it: a target lists a
package in its `npm_deps` as `jsdeps//:<package>` (scopes drop the `@` and
turn `/` into `_`: `jsdeps//:types_lodash`). Rules sync writes `npm_deps`
from the packages a target's sources import.

### `js-deps.toml`

One `[[package]]` per locked package:

```toml
[[package]]
name = "chokidar"
version = "3.6.0"
url = "https://registry.npmjs.org/chokidar/-/chokidar-3.6.0.tgz"
integrity = "sha512-..."
dependencies = ["anymatch", "braces"]
optional_dependencies = ["fsevents"]

[[package]]
name = "fsevents"
version = "2.3.3"
url = "https://registry.npmjs.org/fsevents/-/fsevents-2.3.3.tgz"
integrity = "sha512-..."
os = ["darwin"]
```

- `dependencies` and `optional_dependencies` name the package's
  dependencies (from the lockfile's `snapshots` in pnpm 9).
- `os`, `cpu` and `libc` are the package's own restrictions, as npm names
  them (`darwin`, `x64`, `glibc`, or `!win32` to exclude one). A field
  that's absent allows anything.

### Platform-specific packages

A package whose optional dependencies install on some platforms only, such
as esbuild's per-platform binaries or chokidar's `fsevents`, is resolved in
the cell, for every platform in `buck2.platforms`: its `jsdeps//:<package>`
target is the package plus the optional dependencies the platform gets
(their `os` and `cpu` allow it, and on Linux their `libc` allows glibc), as
a `select()` keyed like rules sync's, with no `DEFAULT`. A consumer lists
the package unconditionally, and each of those packages is linked into its
`node_modules`. `src/examples/typescript-platform-deps` is an example.
