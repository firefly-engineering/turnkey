# Buck2 Cell Generation

Located at `nix/devenv/turnkey/buck2.nix`.

## Toolchains Cell

Generated from `nix/buck2/mappings.nix`.

### Mapping Structure

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
    runtimeDependencies = [ ];
  };
}
```

### Generated Output

`rules.star` is generated with:

1. Load statements for each rule
2. Rule instantiations with configured attributes
3. Visibility set to PUBLIC

### Adding Toolchain Mappings

Edit `nix/buck2/mappings.nix`:

```nix
mylang = {
  skip = false;
  targets = [{
    name = "mylang";
    rule = "system_mylang_toolchain";
    load = "@prelude//mylang:toolchain.bzl";
    visibility = [ "PUBLIC" ];
  }];
};
```

## Dependency Cells

### Go Dependencies

Built by `nix/buck2/go-deps-cell.nix`:

1. Reads go-deps.toml
2. Fetches packages via nix-prefetch
3. Generates rules.star per package

### Rust Dependencies

Built by `nix/buck2/rust-deps-cell.nix`:

1. Reads rust-deps.toml
2. Fetches crates from crates.io
3. Generates rules.star with features

### Python Dependencies

Built by `nix/buck2/python-deps-cell.nix`:

1. Reads python-deps.toml
2. Fetches wheels from PyPI
3. Generates rules.star per package

## Cell Configuration

Each cell gets a `.buckconfig`:

```ini
[cells]
    cellname = .
    prelude = path/to/prelude

[buildfile]
    name = rules.star
```
