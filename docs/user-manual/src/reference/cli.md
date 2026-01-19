# CLI Commands

## tk - Turnkey Buck2 Wrapper

The `tk` command wraps Buck2 with automatic dependency synchronization.

### Usage

```bash
tk [OPTIONS] <COMMAND> [ARGS...]
```

### Options

- `--no-sync` - Skip dependency sync before build commands
- `--verbose` - Enable verbose output

### Commands

All Buck2 commands are supported. Commands that read the build graph trigger auto-sync:

**Auto-sync commands:**
- `build` - Build targets
- `test` - Run tests
- `run` - Run a target
- `targets` - List targets
- `query` - Query the build graph

**Pass-through commands (no sync):**
- `clean` - Clean build outputs
- `kill` - Kill Buck2 daemon
- `help` - Show help

### Examples

```bash
# Build with auto-sync
tk build //...

# Build without sync
tk --no-sync build //...

# Run a target
tk run //path/to:target

# Run tests
tk test //...
```

## godeps-gen

Generate `go-deps.toml` from `go.mod` and `go.sum`.

```bash
godeps-gen > go-deps.toml
```

## rustdeps-gen

Generate `rust-deps.toml` from `Cargo.lock`.

```bash
rustdeps-gen > rust-deps.toml
```

## pydeps-gen

Generate `python-deps.toml` from Python lock files.

```bash
pydeps-gen > python-deps.toml
```
