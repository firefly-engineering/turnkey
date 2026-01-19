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

Buck2 outputs are placed in `buck-out/`:

```
buck-out/
├── v2/
│   └── gen/
│       └── root/
│           └── path/to/target/
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
