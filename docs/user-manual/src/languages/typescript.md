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

## Note on Dependencies

TypeScript/JavaScript dependency management (npm/pnpm) is not yet fully integrated. For now, use genrule for npm-based builds or reference pre-built JavaScript.
