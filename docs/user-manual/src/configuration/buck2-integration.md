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

By default only the `default` shell gets the Buck2 integration: the pinned
buck2 on its PATH, the prelude, and the generated cells. List other shells
in `buck2.shells` to give them the integration too:

```nix
turnkey.toolchains = {
  declarationFiles = {
    default = ./toolchain.toml;
    ci = ./toolchain.ci.toml;
    docs = ./docs/toolchain.toml;   # no Buck2 here
  };
  buck2 = {
    enable = true;
    shells = [ "default" "ci" ];
  };
};
```

Naming a shell that `declarationFiles` doesn't define is an error.

## The buck2 version

Each turnkey revision ships exactly one buck2 release, together with the
prelude built with it. You don't choose buck2 in `toolchain.toml`: you get
a newer buck2 by updating your turnkey input (`nix flake update turnkey`).
Turnkey's test runner speaks buck2's internal test protocol and its prelude
is patched for one prelude revision, so each only works against the release
it was built for
([ADR 0002](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0002-turnkey-owns-the-buck2-version.md)).

To see which release you have, read `turnkey.toolchains.buck2.version`, or
look at the shell's welcome message, which ends with `(buck2 <version>)`
when a `welcomeMessage` is set.

### Migrating from a declared buck2

Earlier turnkey revisions took buck2 from `toolchain.toml`. Declaring it now
fails evaluation:

```text
error: turnkey: /nix/store/…-source/toolchain.toml declares buck2-toolchain, but turnkey now ships buck2 itself.
```

To migrate:

1. Remove the `buck2` or `buck2-toolchain` entry from every `toolchain.toml`.
2. Keep `buck2.enable = true;` in `flake.nix`. If a shell other than
   `default` used to declare buck2, add its name to `buck2.shells`. Before,
   declaring buck2 is what gave a shell the Buck2 integration.
3. `buck2-toolchain` also carried `reindeer`. If you use it, declare
   `reindeer = {}` in `toolchain.toml`.

To run a different buck2 anyway, override turnkey's `toolbox` input with
`follows`. This is unsupported: test result caching or the prelude may break
against another release.

## Generated Cells

When Buck2 integration is enabled, Turnkey generates:

### Toolchains Cell

Located at `.turnkey/toolchains/`, contains toolchain rules for each declared language:

- `toolchains//:go` - Go toolchain
- `toolchains//:rust` - Rust toolchain
- `toolchains//:python` - Python toolchain
- etc.

### Prelude Cell

The Buck2 prelude is provided via Nix at `.turnkey/prelude/`: the prelude built with turnkey's pinned buck2 release, with turnkey's patches and extensions applied.

## A Prelude of Your Own

`prelude.path` replaces turnkey's prelude with a derivation or a path. It is
off the supported path, and it turns test result caching off:

```nix
turnkey.toolchains.buck2.prelude.path = ./my-prelude;
```

## Dependency Cells

Language-specific dependency cells are generated when configured:

- `godeps//` - Go dependencies from go-deps.toml
- `rustdeps//` - Rust dependencies from rust-deps.toml
- `pydeps//` - Python dependencies from python-deps.toml

See [Managing Dependencies](../workflows/dependencies.md) for configuration details.
