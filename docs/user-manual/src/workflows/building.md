# Building Projects

Turnkey integrates with Buck2 for building projects.

## The tk Command

Use `tk` instead of `buck2` directly. It provides:

- Automatic dependency sync before builds
- Consistent behavior across the team

```bash
tk build //path/to:target
```

## Common Build Commands

```bash
# Build a specific target
tk build //src/examples/go-hello:go-hello

# Build all targets
tk build //...

# Build with verbose output
tk build //... -v

# Build in release mode
tk build //... -c release
```

## Build Outputs

Build outputs are placed in `buck-out/.turnkey/`:

```
buck-out/
└── .turnkey/
    ├── gen/
    │   └── root/
    │       └── path/to/target/
    └── tmp/
        └── ...
```

**Why `.turnkey`?** The isolation directory starts with a dot so that language tools ignore it:
- Go skips directories starting with `.` when scanning for packages
- Cargo ignores dot-directories
- pytest ignores dot-directories by default

This prevents errors like Go trying to parse generated `.go` files in build outputs, or pytest collecting test files from there.

To find the output path for a specific target:

```bash
tk build //path/to:target --show-output
```

## Skipping Sync

If you know dependencies haven't changed:

```bash
tk --no-sync build //...
```

## Troubleshooting

### Missing Toolchain

If you see "toolchain not found", ensure:
1. The toolchain is declared in `toolchain.toml`
2. You've re-entered the shell after adding it

### Stale Dependencies

If builds fail with missing dependencies:
```bash
tk sync
tk build //...
```
