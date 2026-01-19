# Buck2 Integration

Turnkey provides first-class Buck2 integration with automatic toolchain and dependency cell generation.

## Enabling Buck2

In your `flake.nix`:

```nix
turnkey.toolchains = {
  enable = true;
  declarationFiles.default = ./toolchain.toml;
  buck2.enable = true;
};
```

## Generated Cells

When Buck2 integration is enabled, Turnkey generates:

### Toolchains Cell

Located at `.turnkey/toolchains/`, contains toolchain rules for each declared language:

- `toolchains//:go` - Go toolchain
- `toolchains//:rust` - Rust toolchain
- `toolchains//:python` - Python toolchain
- etc.

### Prelude Cell

The Buck2 prelude is provided via Nix at `.turnkey/prelude/`. This ensures reproducible builds with a pinned prelude version.

## Prelude Strategies

```nix
turnkey.toolchains.buck2.prelude = {
  strategy = "nix";  # default, recommended
  # Other options: "bundled", "git", "path"
};
```

- **nix** (default): Uses Turnkey's Nix-backed prelude with custom extensions
- **bundled**: Uses Buck2's built-in prelude
- **git**: Uses a git checkout
- **path**: Uses a local filesystem path

## Dependency Cells

Language-specific dependency cells are generated when configured:

- `godeps//` - Go dependencies from go-deps.toml
- `rustdeps//` - Rust dependencies from rust-deps.toml
- `pydeps//` - Python dependencies from python-deps.toml

See [Managing Dependencies](../workflows/dependencies.md) for configuration details.
