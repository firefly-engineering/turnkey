# Adding Toolchains

This guide covers adding new toolchains to Turnkey.

## Steps

1. Add package to registry
2. Add mapping to mappings.nix
3. (Optional) Create prelude extension

## 1. Add to Registry

Edit `nix/registry/default.nix`:

```nix
{
  # Existing entries...

  # Add your toolchain
  zig = pkgs.zig;
}
```

## 2. Add Toolchain Mapping

Edit `nix/buck2/mappings.nix`:

### For Standard Toolchains

```nix
zig = {
  skip = false;
  targets = [{
    name = "zig";
    rule = "system_zig_toolchain";
    load = "@prelude//zig:toolchain.bzl";
    visibility = [ "PUBLIC" ];
  }];
  implicitDependencies = [ ];
};
```

### For Non-Toolchain Tools

Some tools don't need Buck2 rules:

```nix
mydevtool = {
  skip = true;
  reason = "Development utility, not a Buck2 toolchain";
};
```

## 3. Dynamic Attributes

For toolchains needing Nix store paths:

```nix
mylang = {
  targets = [{
    name = "mylang";
    rule = "system_mylang_toolchain";
    load = "@prelude//mylang:toolchain.bzl";
    dynamicAttrs = registry: {
      compiler = "${registry.mylang}/bin/mycompiler";
    };
  }];
};
```

## 4. Prelude Extension (if needed)

If the upstream prelude doesn't have rules for your language, create a prelude extension. See [Prelude Extensions](./prelude-extensions.md).

## Testing

1. Add toolchain to `toolchain.toml`
2. Stage files: `git add nix/`
3. Enter shell: `nix develop`
4. Verify: `tk targets toolchains//...`
