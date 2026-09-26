# Turnkey

Turnkey is toolchain management for Nix flakes. You declare the tools a
project needs in a small `toolchain.toml`; turnkey resolves them to pinned Nix
packages and puts them in a [devenv](https://devenv.sh/) shell. For projects
built with [Buck2](https://buck2.build/), it also ships a pinned buck2 release
and prelude, and turns each language's native dependency files (`go.mod`,
`Cargo.lock`, `uv.lock`, `pnpm-lock.yaml`, ...) into Buck2 dependency cells
built by Nix.

Turnkey is a library, not an application: projects import it as a flake input
and configure it through its [flake-parts](https://flake.parts/) module. This
repository uses itself that way, so its own `flake.nix` and `toolchain.toml`
are a working example.

Supported languages: Go, Rust, Python, TypeScript, Solidity and Jsonnet.

## Getting started

### Prerequisites

- **Nix** with flakes enabled. If they aren't, add this to
  `~/.config/nix/nix.conf`:

  ```
  experimental-features = nix-command flakes
  ```

- **direnv** (recommended), hooked into your shell, so the environment loads
  when you enter the project.

### A new project

Start from the template, which sets up a Go project built with Buck2:

```bash
mkdir my-project && cd my-project
nix flake init -t github:firefly-engineering/turnkey
direnv allow        # or: nix develop
```

### An existing project

Add turnkey, flake-parts and devenv to your flake inputs, import the modules
and point turnkey at your `toolchain.toml`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
        inputs.turnkey.flakeModules.turnkey
      ];

      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem = { ... }: {
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          # Optional: turnkey's pinned buck2, plus a Go dependency cell
          buck2 = {
            enable = true;
            go = {
              enable = true;
              depsFile = ./go-deps.toml;
            };
          };
        };
      };
    };
}
```

```toml
# toolchain.toml
[toolchains]
go = {}
```

Don't declare `buck2` in `toolchain.toml`: `buck2.enable` adds turnkey's
pinned release. Each `buck2.<language>.enable` puts that language's
dependency generator on `PATH` and registers its cell with Buck2.

Then enter the shell with `direnv allow` or `nix develop`, and build:

```bash
tk build //...
```

The [user manual](https://firefly-engineering.github.io/turnkey/user-manual/)
covers [installation](docs/user-manual/src/getting-started/installation.md),
[project setup](docs/user-manual/src/getting-started/project-setup.md), the
full [`toolchain.toml` reference](docs/user-manual/src/configuration/toolchain-toml.md)
and each language.

## Tools

A turnkey shell provides two CLI tools.

### `tk`: the Buck2 wrapper

`tk` wraps `buck2`. Before any command that reads the build graph (`build`,
`test`, `run`, `query`, ...), it syncs the generated files the graph depends
on: dependency cells such as `go-deps.toml`, and `rules.star` files. Other
commands (`clean`, `kill`, ...) pass straight through.

```bash
tk build //some:target    # sync, then build
tk test //...             # sync, then test
tk sync                   # sync stale files explicitly
tk check                  # report stale files without changing them (for CI)
tk --no-sync build //...  # skip the dependency sync (--no-rules-sync skips rules.star)
```

What gets synced, and from which inputs, is configured in
`.turnkey/sync.toml`.

### `tw`: the native tool wrapper

`tw` wraps the language's own tools (`go`, `cargo`, `uv`). It records the
dependency files before the command runs and, if the command changed them,
runs the matching sync, so adding a dependency the usual way keeps the Buck2
side current.

```bash
tw go get github.com/pkg/errors
tw cargo add serde
tw uv add requests
tw -v go get ...          # show what tw detects and syncs
```

In a turnkey shell the `go`, `cargo` and `uv` on `PATH` are thin wrappers that
call `tw` for you, so a plain `go get` does the same. Set `TURNKEY_NO_WRAP=1`
to run the real tool directly.

The [CLI reference](docs/user-manual/src/reference/cli.md) documents both tools
in full.

## Documentation

- [User manual](https://firefly-engineering.github.io/turnkey/user-manual/)
  ([source](docs/user-manual/src/SUMMARY.md)): using turnkey in a project
- [Developer manual](https://firefly-engineering.github.io/turnkey/developer-manual/)
  ([source](docs/developer-manual/src/SUMMARY.md)): working on turnkey itself
- [Architecture decisions](docs/adr/)

## Working on turnkey

Clone the repository and run `direnv allow` in it; the dev shell provides every
toolchain turnkey itself uses.

Work is tracked in [GitHub Issues](https://github.com/firefly-engineering/turnkey/issues), prioritised in the [turnkey project](https://github.com/orgs/firefly-engineering/projects/3).

- **Find work**: `gh issue list --search "-is:blocked no:assignee"`
- **Claim work**: `gh issue edit <n> --add-assignee @me`
- **Submit work**: commit with `Fixes #<n>`, then push

See [`AGENTS.md`](AGENTS.md) for detailed workflow instructions.
