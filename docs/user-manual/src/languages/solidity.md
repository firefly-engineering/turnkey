# Solidity Support

Turnkey provides Solidity smart contract support with Buck2 integration, including compilation, testing with Foundry, and dependency management.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
solidity = {}
foundry = {}
```

Enable Solidity dependencies in `flake.nix` (if using external libraries):

```nix
turnkey.toolchains.buck2.solidity = {
  enable = true;
  depsFile = ./solidity-deps.toml;
};
```

## Project Structure

```
my-project/
├── foundry.toml              # The only Foundry configuration, for the whole repository
├── solidity-deps.toml        # Generated dependency manifest
├── remappings.txt            # Generated from solidity-deps.toml
└── src/
    └── contracts/
        ├── rules.star
        ├── src/
        │   └── MyToken.sol
        └── test/
            └── MyToken.t.sol
```

The repository has at most one `foundry.toml`, at its root. A package under
`src/` has none of its own: a nested `foundry.toml` would become forge's root for its
subtree and break native `forge` there. With the `tk.foundryConfigCheck` option
on, a pre-commit hook rejects one (see [Native forge](#native-forge)).

## Build Rules

forge drives both `solidity_library` and `solidity_test`, and reads the same
configuration native forge reads. The root `foundry.toml` holds every compiler
and test setting (optimizer, `optimizer_runs`, `evm_version`, `via_ir`,
`[fuzz] runs`, ...), and the root `remappings.txt` every remapping (overrides
go in `foundry.toml`, see [Remappings](#remappings)). The rules take neither
settings nor remappings as attributes. So tests run against the bytecode that
ships, and native `forge build` produces the same bytecode as Buck2.

Each Solidity action runs forge in a scratch project that holds only its
declared inputs, each at its place in the repository:

- the root `foundry.toml` and `remappings.txt`;
- the target's sources, and those of every `solidity_library` it depends on;
- the `soldeps` cell's `bundle` (every vendor package), at the cell link's
  path, `.turnkey/soldeps/`, where the root `remappings.txt` points.

forge runs with the toolchain's `solc` (`--use`), `--offline`, and without the
caller's `FOUNDRY_*`/`DAPP_*` variables or `~/.foundry` configuration.

The `solidity_library` and `solidity_test` macros add these inputs
themselves, from the `[solidity]` section turnkey writes into the generated
`.buckconfig`: `foundry.toml` always, the bundle and `remappings.txt` when
the repository has a `soldeps` cell. `foundry.toml` must be at the repository
root (`turnkey.toolchains.buck2.solidity.foundryTomlFile`'s default); the
shell fails to evaluate otherwise.

> **Breaking change:** a repository using the Solidity rules must export both
> files from a `rules.star` at its root:
>
> ```python
> # rules.star, at the repository root
> export_file(name = "foundry.toml", visibility = ["PUBLIC"])
> export_file(name = "remappings.txt", visibility = ["PUBLIC"])
> ```
>
> Without them, a Solidity target fails to build with an unknown
> `root//:foundry.toml` target. The rules also no longer accept `optimizer`,
> `optimizer_runs`, `fuzz_runs`, `solc_version` or `remappings`: set them in
> the root `foundry.toml`.

Changing a setting in `foundry.toml`, or a dependency, rebuilds and retests
every Solidity target.

### solidity_library

Compile Solidity source files with `forge build <srcs>`:

```python
load("@prelude//solidity:solidity.bzl", "solidity_library")

solidity_library(
    name = "my_token",
    srcs = ["src/MyToken.sol"],
    deps = ["soldeps//:openzeppelin_contracts"],
)
```

Its output is forge's artifacts directory (`out/`): one
`<File>.sol/<Contract>.json` per contract compiled, imports included.

### solidity_contract

Extract a specific contract from a compiled library:

```python
load("@prelude//solidity:solidity.bzl", "solidity_contract")

solidity_contract(
    name = "my_token_artifact",
    contract = "MyToken",  # Contract name in source
    lib = ":my_token",
)
```

It takes the artifact of the contract of that name defined in one of the
library's own sources (not in an import); if two of them define it, split the
library. It produces what solc's own outputs would be:

- `{contract}.abi` - Contract ABI (JSON)
- `{contract}.bin` - Deployment bytecode (hex)
- `{contract}.bin-runtime` - Runtime bytecode (hex)
- `{contract}.metadata.json` - Compiler metadata

### solidity_test

Run tests with Foundry's `forge test`:

```python
load("@prelude//solidity:solidity.bzl", "solidity_test")

solidity_test(
    name = "my_token_test",
    srcs = ["test/MyToken.t.sol"],
    deps = [":my_token"],
)
```

Fuzz runs come from `foundry.toml`'s `[fuzz] runs`. The fuzz seed is fixed per
target, unless the target opts out of test result caching (see
[Test result caching](../workflows/test-result-caching.md)).

## External Dependencies

`tk sync` collects Solidity dependencies from two places, the root
`foundry.toml` and the root `package.json`, into the generated
`solidity-deps.toml`, and generates the root `remappings.txt` from that. Both
files are committed. Only direct dependencies count: `tk sync` does not follow a
dependency's own `[dependencies]`, submodules or `remappings.txt`, so anything a
dependency needs is declared at the root too.

### Git dependencies

Declare git dependencies in the root `foundry.toml`'s `[dependencies]`, in
[Soldeer](https://github.com/mario-eth/soldeer)'s table form:

```toml
[dependencies]
forge-std = { version = "1.8.0", git = "https://github.com/foundry-rs/forge-std", tag = "v1.8.0" }
solady = { version = "0.1.26", git = "https://github.com/vectorized/solady", rev = "<40-hex commit>" }
```

- `version` is required. It is the package's version in `solidity-deps.toml`,
  and what [fixup sets](../workflows/fixups.md) match on.
- `git` is the repository URL.
- The pin is exactly one of `tag`, `branch` or `rev`. A `rev` must be a full
  40-character commit hash: `tk sync` resolves pins with `git ls-remote`, which
  lists refs but cannot expand an abbreviated commit without a clone. Pin a
  name with `tag` or `branch` instead.

Turnkey never runs Soldeer; `soldeps-gen` reads the table itself. A string
value, such as a Soldeer registry version (`forge-std = "1.9.7"`) or the older
`"<url>@<ref>"` form, is rejected with an error showing the table form. So is
Soldeer's `url = "…"` form, since URL dependencies are not supported, and any
other unknown key, such as a misspelt pin.

### npm packages

Solidity packages published to npm, such as `@openzeppelin/contracts`, are
ordinary dependencies in the root `package.json`, at the versions and integrity
hashes `pnpm-lock.yaml` pins. A `package.json` dependency counts as a Solidity
dependency when its tarball contains `.sol` files; there is no list of known
packages. To tell, `tk sync` downloads the tarball, checks it against the lock's
`integrity` (and fails on a mismatch), and looks for `.sol` files, within
generous size limits. `solidity-deps.toml` remembers the verdict for each lock
`integrity`, or for each name and version when the lock records no integrity:
a Solidity package is recorded as a `[[package]]`, any other dependency as a
`[[not_solidity]]` entry. A recorded verdict is always reused, so a sync only
downloads the tarballs of packages that are new or that changed. When a
download fails, or `--no-prefetch` rules it out, the sync fails instead of
guessing.

### Using a dependency

Reference a package through the `soldeps` cell:

```python
solidity_library(
    name = "my_token",
    srcs = ["MyToken.sol"],
    deps = ["soldeps//:openzeppelin_contracts"],
)
```

and import it under its own name:

```solidity
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "forge-std/Test.sol";
```

### Syncing

`tk sync` regenerates `solidity-deps.toml` whenever `foundry.toml`,
`package.json` or `pnpm-lock.yaml` changes (the paths are the
`turnkey.toolchains.buck2.solidity` options `foundryTomlFile`, `packageJsonFile`
and `pnpmLockFile`), then the root `remappings.txt`, next to `foundry.toml`,
from `solidity-deps.toml`. It runs `soldeps-gen`, which prefetches: it pins each
git dependency to the commit its tag, branch or rev resolves to. For a GitHub
repository it also records that commit's source archive and its Nix hash, so the
`soldeps` cell fetches it as a fixed-output derivation. An npm package that
`pnpm-lock.yaml` gives no integrity for gets the hash of its tarball. Hashes go
through turnkey's prefetch cache, so a regeneration only fetches what changed.

### Remappings

Each package is imported under its own name, `<name>/`, with no aliases:
`forge-std/...` resolves inside forge-std. An npm package maps to its root. A
git dependency maps to the directory its own `foundry.toml` names as
`[profile.default] src`, to forge's default `src/` when its `foundry.toml` sets
none, and to the repository root when it has no `foundry.toml`. `soldeps-gen`
reads that file at the pinned commit while prefetching, and records the result
as the package's `remapping` in `solidity-deps.toml`. Later syncs reuse the
recorded target while the package's pin is unchanged, so a sync with
`--no-prefetch` works for them; a new or re-pinned git dependency needs a
prefetching sync.

`soldeps-gen` reads a dependency's `foundry.toml` only from GitHub. For a git
dependency hosted elsewhere, the sync fails rather than guess; give it an
override (below) naming where its sources live.

To point a package elsewhere, add a remapping to the root `foundry.toml`:

```toml
[profile.default]
remappings = ["solady/=.turnkey/soldeps/vendor/solady/src/"]
```

Its prefix must be `<name>/` of a declared package (remappings cannot alias
one package under another name), and its target must lie inside
`.turnkey/soldeps/vendor/<name>/`. `tk sync` rejects anything else.

The root `remappings.txt` sits next to `foundry.toml` and holds every
package's remapping, overrides included, with targets under the `soldeps` cell
link. Forge resolves targets from `foundry.toml`'s directory, so with the
file at the project root they read:

```
@openzeppelin/contracts/=.turnkey/soldeps/vendor/@openzeppelin/contracts/
forge-std/=.turnkey/soldeps/vendor/forge-std/src/
```

With a `foundry.toml` in a subdirectory, the targets (and the overrides you
write) start with one `../` per level instead. Forge prefers `remappings.txt`
to the `remappings` key, and the two agree because the file already contains
the overrides. Don't edit the file; change
`foundry.toml` and run `tk sync`.

## Native forge

`forge build` and `forge test` work in the dev shell alongside `tk build` and
`tk test`, the same way running `cargo` or `go` natively does. They run from any
directory, since forge finds the root `foundry.toml`, and cover every package
under `src/`:

```toml
[profile.default]
src = "src"
test = "src"
libs = [".turnkey/soldeps/vendor"]
out = "out"
auto_detect_remappings = false
optimizer = true
optimizer_runs = 200

[fuzz]
runs = 256
```

`src` and `test` are both `src`, so Solidity code added anywhere under `src/` is
covered without editing the file. There is no per-package scoping; narrow a run
with `--match-path` instead:

```bash
forge build
forge test --match-path 'src/contracts/**'
```

Git-ignore forge's `/out/` and `/cache/`.

Imports resolve through the root `remappings.txt` that `tk sync` generates
(see [Remappings](#remappings)), into the `soldeps` cell's `vendor/` directory
(`libs`). Automatic remapping detection is off, so forge does not guess
remappings of its own.

**The compiler comes from the dev shell.** `foundry.toml` sets neither `solc`
nor `solc_version`. When Solidity is enabled, the dev shell exports
`FOUNDRY_SOLC`, the solc the Buck2 toolchain uses, together with
`FOUNDRY_OFFLINE=true`. Native runs therefore share their compiler with the
Buck2 rules and never download one; the other settings are shared through
`foundry.toml` itself (see [Build Rules](#build-rules)). The toolchain
declared in `toolchain.toml` stays the one place the compiler version is set;
a `solc_version` in `foundry.toml` could only go stale on a toolchain bump, so
the pre-commit hook rejects `solc` and `solc_version` keys.

## Compiler Version

The compiler version comes from the toolchain declared in `toolchain.toml`
(`solidity-toolchain` or `solc`, not both): Buck2's Solidity rules and native forge
(through `FOUNDRY_SOLC`) both use its `solc`. To change the version, change the
toolchain.

There is one compiler per repository: the rules take no per-target version.

## Building and Testing

```bash
# Build contracts
tk build //src/contracts:my_token

# Run tests
tk test //test:my_token_test

# Build all Solidity targets
tk build //... --target-platforms //platforms:solidity
```

## Forge Integration

The `solidity_test` rule wraps Foundry's `forge test`, supporting:
- Unit tests
- Fuzz testing
- Fork testing (with `fork_url` attribute)
- Gas reports

```python
solidity_test(
    name = "integration_test",
    srcs = ["Integration.t.sol"],
    deps = [":my_token"],
    fork_url = "https://eth-mainnet.g.alchemy.com/v2/...",  # Optional
)
```
