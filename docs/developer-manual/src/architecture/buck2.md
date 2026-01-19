# Buck2 Integration

Turnkey generates Buck2 cells at shell entry time.

## Generated Cells

### Toolchains Cell (`.turnkey/toolchains/`)

Contains toolchain rules for each declared language:

```python
# Generated rules.star
load("@prelude//toolchains/go:system_go_toolchain.bzl", "system_go_toolchain")

system_go_toolchain(
    name = "go",
    visibility = ["PUBLIC"],
)
```

### Prelude Cell (`.turnkey/prelude/`)

Symlink to Nix-built prelude with:
- Upstream buck2-prelude at pinned commit
- Applied patches
- Custom extensions (TypeScript, mdbook, etc.)

### Dependency Cells

- `godeps/` - Go third-party packages
- `rustdeps/` - Rust crates
- `pydeps/` - Python packages

## Toolchain Mappings

Located at `nix/buck2/mappings.nix`:

```nix
{
  go = {
    skip = false;
    targets = [{
      name = "go";
      rule = "system_go_toolchain";
      load = "@prelude//toolchains/go:system_go_toolchain.bzl";
      visibility = [ "PUBLIC" ];
    }];
    implicitDependencies = [ "python" "cxx" ];
  };
}
```

## Generation Process

1. Devenv shell entry hook runs
2. `nix/devenv/turnkey/buck2.nix` generates toolchains cell
3. Dependency cells built from deps files
4. Symlinks created in `.turnkey/`

## Prelude Customization

The prelude is built by `nix/buck2/prelude.nix`:

1. Fetch upstream buck2-prelude
2. Apply patches from `nix/patches/prelude/`
3. Copy extensions from `nix/buck2/prelude-extensions/`

See [Prelude Extensions](../extending/prelude-extensions.md) for adding custom rules.
