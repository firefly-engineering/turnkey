# Managing Dependencies

Turnkey manages external dependencies through language-specific dependency files.

## Dependency Flow

1. Native lock files (`go.sum`, `Cargo.lock`, `uv.lock`)
2. Turnkey deps files (`go-deps.toml`, `rust-deps.toml`, `python-deps.toml`)
3. Buck2 cells (`godeps//`, `rustdeps//`, `pydeps//`)

## Auto-Sync with Wrapped Tools

When using `go`, `cargo`, or `uv`, Turnkey automatically syncs dependencies:

```bash
# These trigger auto-sync when dependency files change
go get github.com/some/package
cargo add serde
uv add requests
```

## Manual Sync

Force a dependency sync:

```bash
tk sync
```

## Go Dependencies

Configure in `flake.nix`:

```nix
turnkey.toolchains.buck2.go = {
  enable = true;
  depsFile = ./go-deps.toml;
};
```

Generate `go-deps.toml`:

```bash
godeps-gen > go-deps.toml
```

## Rust Dependencies

```nix
turnkey.toolchains.buck2.rust = {
  enable = true;
  depsFile = ./rust-deps.toml;
};
```

Generate `rust-deps.toml`:

```bash
rustdeps-gen > rust-deps.toml
```

## Python Dependencies

```nix
turnkey.toolchains.buck2.python = {
  enable = true;
  depsFile = ./python-deps.toml;
};
```

Generate `python-deps.toml`:

```bash
pydeps-gen > python-deps.toml
```
