# Dependency Fixups

Some dependencies don't build under Buck2 as they are. A Rust crate's
`build.rs` never runs, so whatever it generates, compiles or tells rustc has
to come from somewhere else; a dependency may need a patch. A **fixup** is
what turnkey supplies for one dependency to make it build: a patch, the
output its build script would generate, the flags it would pass.

Fixups come in **fixup sets**: modules of class `turnkeyFixups`
([ADR 0003](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0003-fixup-sets-are-modules.md)).
Your repository brings the sets it needs, and its own fixups, through one
option. turnkey applies no fixups you didn't bring.

## Bringing fixups

```nix
perSystem = { ... }: {
  turnkey.toolchains.buck2.fixups = {
    # Sets published by other flakes
    imports = [
      inputs.turnkey.modules.turnkeyFixups.serde
      inputs.acme-fixups.modules.turnkeyFixups.default
    ];

    # This repository's own fixups
    rust.zerocopy.buildScript.skip = true;
  };
};
```

A fixup is keyed by the dependency's name in its own ecosystem:
`rust.<crate>`, `go."<import path>"`, `python.<distribution>`,
`javascript.<package>`, `solidity.<package>`.

### turnkey's published fixups

turnkey publishes the fixups its own repository needs, one module per
family, under `inputs.turnkey.modules.turnkeyFixups`:

| Module | Crates |
| ------ | ------ |
| `serde` | serde, serde_core, serde_json |
| `thiserror` | thiserror |
| `ring` | ring 0.17 |
| `rustix` | rustix |
| `nix` | nix |
| `fuser` | fuser |
| `tree-sitter` | tree-sitter and its rust, python, solidity, starlark and typescript grammars |
| `build-script-skips` | crates whose build script turnkey's builds need nothing from |
| `default` | all of the above |

Import only what your lock needs: an imported fixup for a crate you don't
lock is silently unused.

## Every build script needs a fixup

Buck2 never runs `build.rs`, so every crate you lock that has one needs a
fixup saying what stands in for it. The Rust cell fails to build otherwise:

```
error: turnkey: serde 1.0.228 has a build.rs, and no fixup says what stands in for it; a published fixup set does: add `inputs.turnkey.modules.turnkeyFixups.serde` to turnkey.toolchains.buck2.fixups.imports
```

When no published set accounts for the crate, the error says how to write
the fixup. If the crate's build script only probes the compiler or the
target, or emits cfgs for features you don't use, the build needs nothing
from it:

```nix
rust.zerocopy.buildScript.skip = true;
```

Otherwise, give it a build script generating what its `build.rs` would, and
the flags it would pass. The developer manual's
[Dependency Generators](../../../developer-manual/src/extending/dependency-generators.md)
page describes the whole record and how to diagnose what a crate needs.

## Versions

A fixup applies to every locked version of its dependency. Fields that
hold only for some versions go in `versions` entries, whose bounds are
compared with the locked version:

```nix
rust.ring.versions = [
  {
    when = { atLeast = "0.17"; below = "0.18"; };
    buildScript.generate = ...;
  }
];
```

Every entry whose bounds hold applies. A version no entry covers gets only
the fixup's other fields, so a new major version of a crate is reported as
unaccounted for rather than built with a fixup written for another.

## Patches

```nix
rust.some-crate.patches = [ ./patches/some-crate-fix.patch ];
go."github.com/foo/bar".patches = [ ./patches/bar.patch ];
```

A fixup's patches apply to the dependency's own source, in order, with
`-p1`: a plain `git diff` in a checkout of the dependency works as is.
Patches from several sets apply in import order, before a Rust crate's
build script runs.

Fixup patches are separate from the patches `tk compose patch` writes to
`.turnkey/patches/<cell>/` from the FUSE edit layer. Those are this
repository's local, exact-version workarounds, and apply last, to the
assembled cell.

## When sets disagree

Fixups merge field by field, as NixOS modules do: flags and patches from
every set concatenate. Two sets that give a crate different build scripts,
or an environment variable different values, fail evaluation, naming both
files:

```
error: The option `rust.serde.buildScript.generate.<function body>' has conflicting definition values:
- In `conflicting/flake.nix#modules.turnkeyFixups.default': "echo another serde"
- In `acme/flake.nix#modules.turnkeyFixups.default': "..."
```

Resolve it in your own fixups:

- Override one field with `lib.mkForce`:
  `rust.serde.buildScript.generate = lib.mkForce "...";`
- Drop one fixup an imported set brings: `rust.ring.enable = false;`
- Drop a whole module: `disabledModules = [ ... ];`, or don't import it.

## Unused fixups

A fixup written in your own `turnkey.toolchains.buck2.fixups` that matches
no locked dependency, or a `versions` entry matching none of its locked
versions, warns at evaluation: it usually means a rename or an upgrade left
it behind. Fixups from imported sets never warn, so one organization-wide
set can serve many repositories that each lock only part of it.

## Publishing a fixup set

A fixup set is a plain module; publish it from any flake as
`modules.turnkeyFixups.<name>`. With flake-parts, import its `modules`
module, which stamps the module's class:

```nix
{
  imports = [ inputs.flake-parts.flakeModules.modules ];

  flake.modules.turnkeyFixups = {
    openssl = ./fixups/openssl.nix;
    default = { imports = [ ./fixups/openssl.nix ]; };
  };
}
```

Without flake-parts, set the class yourself:

```nix
outputs = { self, ... }: {
  modules.turnkeyFixups.default = {
    _class = "turnkeyFixups";
    imports = [ ./fixups/openssl.nix ];
  };
};
```

A set's module receives `pkgs` and `lib`, and may hold fixups for several
languages at once, such as for a library packaged for both Rust and
Python:

```nix
# fixups/acme-proto.nix
{ ... }:
{
  rust.acme-proto = {
    buildScript.skip = true;
    patches = [ ./acme-proto-rust.patch ];
  };
  python.acme-proto.patches = [ ./acme-proto-python.patch ];
}
```
