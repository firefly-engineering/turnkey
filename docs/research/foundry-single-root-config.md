# Can one root `foundry.toml` serve every Solidity source?

Research for [#40](https://github.com/firefly-engineering/turnkey/issues/40),
part of map [#83](https://github.com/firefly-engineering/turnkey/issues/83)
(hermetic soldeps cell and a single root `foundry.toml`). Gathered on
2026-09-26 from:

- the Foundry source at tag `v1.5.1`, the version turnkey's
  `solidity-toolchain` v2 pins (`toolchain.toml:14`);
- `foundry-compilers` 0.19.6 and `soldeer-core` 0.9.0, the versions that
  `foundry` v1.5.1's `Cargo.lock` resolves;
- the Foundry book at its current `main`;
- the Solidity 0.8.34 documentation;
- turnkey's own code at `main` (`1f0fca95`).

Every behaviour marked **(verified)** was reproduced with `forge 1.5.1`
(`b0a9dd9c`) and `solc 0.8.34` from the devshell, in scratch projects under
`/tmp`. *(inferred)* marks a conclusion drawn from sources rather than stated
in them. *(unverified)* marks something no source settles.

## TL;DR

- **Yes, one root file works.** It needs three settings:
  - `src`/`test` set to a directory that contains every package, e.g.
    `src = "src"`;
  - `libs` pointing at the soldeps cell;
  - an explicit remapping list.

  `forge build`/`forge test` then compile and run every example from the
  repo root, and from any subdirectory, because `forge` walks up to the
  nearest `foundry.toml` inside the git repo **(verified)**.
- **`src`, `test` and `script` are each one path, not a list or a glob.** A
  glob matches nothing **(verified)**. To cover many packages you point them
  at a common ancestor. `libs` is the only list.
- **What a single root breaks:**
  - Running `forge` inside an example no longer scopes it to that example.
    It runs every test in the repo **(verified)**. Scoping takes
    `--match-path 'src/examples/x/**'`, which is relative to the repo root,
    not the cwd **(verified)**, or a `FOUNDRY_PROFILE`.
  - One `out/` and one `cache/` at the root. Neither is in `.gitignore`
    today.
  - One set of compiler settings. Per-file overrides go through
    `compilation_restrictions`.
- **Per-example files cannot just `extends` the root.** `extends` merges
  values, but relative paths (`libs`, `remappings`) are re-anchored at the
  *child's* root, and `remappings.txt` is only read from the config root
  **(verified)**. A thin per-package file must restate its paths.
- **Remapping auto-detection is wrong for the soldeps layout.** Pointed at
  the cell's `vendor/`, it produces `@openzeppelin/=…/@openzeppelin/contracts/`,
  which doubles the `contracts/` segment **(verified)**. The root must set
  `auto_detect_remappings = false` and list its remappings explicitly.
- **Turnkey's `[dependencies]` table squats on Soldeer's schema.** Foundry
  parses `[dependencies]` as Soldeer config. Soldeer reads a string value as
  a *registry version requirement*, so `forge-std = "https://…@v1.8.0"`
  means nothing sensible to `forge soldeer`.
- **Buck2 never reads `foundry.toml`.**
  - `solidity_library` calls `solc` with remappings taken from the soldeps
    cell's `remappings.txt`.
  - `solidity_test` writes its own throwaway `foundry.toml`.
  - Only `soldeps-gen` (the `[dependencies]` table), the pre-commit check
    and the rules mapper (existence only) touch the file.
- **Two bugs found on the way:**
  1. The cell's `remappings.txt` maps `forge-std/` to `vendor/forge-std/`,
     dropping soldeps-gen's `src/`. `import "forge-std/Test.sol"` fails
     under Buck2 **(verified with `solc`)**.
  2. The per-example files use `fuzz_runs`, a key forge 1.5.1 rejects as
     unknown **(verified)**. They also pin `solc_version = "0.8.33"` against
     a 0.8.34 toolchain.

## 1. Foundry configuration semantics

### Discovery: which `foundry.toml`, which root

- **Upward search, bounded by git.** `find_project_root` walks the cwd's
  ancestors and takes the first directory holding a `foundry.toml`. It does
  not look above the directory containing `.git`. If it finds no file, it
  returns the git root, or failing that the cwd [\[f-root\]][f-root]
  **(verified: from `src/examples/a`, `forge config` resolves the repo-root
  file)**.
  - The book says the search runs "until the file is found or the root is
    reached" [\[book-overview\]][book-overview]. It does not mention the git
    boundary. The source is authoritative here.
  - *(inferred)* In a secondary jj workspace, which has no `.git`, the
    boundary disappears. This only matters when no `foundry.toml` exists up
    the tree.
- **The nearest file wins outright.** There is no merging of nested files: a
  `foundry.toml` in `src/examples/b` makes `b` the root, and the repo-root
  file is ignored [\[f-root\]][f-root] **(verified)**.
- **Explicit overrides.**
  - `--root <dir>` and `--config-path <file>` pick the root directly. The
    config path must end in `foundry.toml`, and its parent becomes the root
    [\[f-cli-root\]][f-cli-root].
  - `FOUNDRY_CONFIG` swaps the file read but keeps the discovered root
    [\[f-figment\]][f-figment].
- **A global file is merged first.** `~/.foundry/foundry.toml` is merged
  before the project file [\[f-figment\]][f-figment]. *(inferred)* That
  makes it an unhermetic input for native `forge` runs. It does not affect
  Buck2, whose `solidity_test` runs in a temp dir but still inherits `$HOME`
  *(unverified whether the test runner sets `HOME`)*.

### Paths: `src`, `test`, `script`, `out`, `libs`

- `src`, `test`, `script` and `out` are each a single `PathBuf`. `libs` is
  a `Vec<PathBuf>` [\[f-fields\]][f-fields]. All of them are relative to the
  project root [\[book-project\]][book-project].
- **No globs.** `FOUNDRY_SRC='src/examples/*/src'` compiles nothing
  ("Nothing to compile") **(verified)**.
- **What gets compiled.** The compile set is every `.sol` file under `src`,
  `test` and `script`, recursively [\[fc-inputs\]][fc-inputs].
  `forge test` without a filter compiles `src` ∪ `test` and skips scripts
  [\[f-test-sources\]][f-test-sources].
- **How tests are found.** Test contracts are recognised by their contents,
  not by being under `test`. The test filter only excludes paths under a
  `libs` directory [\[f-test-filter\]][f-test-filter]. So
  `src = "src"`, `test = "src"` runs tests found anywhere under `src/`
  **(verified: both examples' suites ran from one root)**.
- **Nearby keys.** `allow_paths` and `include_paths` feed solc's
  `--allow-paths` and `--include-path`. The root and every `libs` entry are
  allowed automatically [\[f-paths\]][f-paths].

### Profiles and `FOUNDRY_PROFILE`

- `FOUNDRY_PROFILE` selects `[profile.<name>]`, and the default is
  `default` [\[f-profile\]][f-profile].
- A non-default profile is layered on `[profile.default]`, so it inherits
  everything it does not override
  [\[f-merge-toml\]][f-merge-toml], [\[book-profiles\]][book-profiles].
- Paths in a profile are still relative to the one project root, so a
  profile can narrow `src`/`test` to one package without any re-anchoring
  problem. **(verified:** `FOUNDRY_PROFILE=hello-a` with
  `src = "src/examples/a/src"` ran only `a`'s suite, from inside
  `src/examples/b`).

### `extends` (config inheritance)

- **Syntax.** `extends = "path"` or
  `extends = { path, strategy }`, with strategies `extend-arrays` (the
  default), `replace-arrays` and `no-collision`
  [\[f-extend\]][f-extend], [\[book-project\]][book-project].
- **Where it lives and how it resolves.**
  - `extends` is read from the **selected profile only**, so a file that
    wants inheritance under several profiles repeats it in each.
  - The path is resolved against the including file's directory.
  - A base file may not itself extend another [\[f-ext-read\]][f-ext-read].
- **What it merges.** It merges TOML *values* before the root is applied.
  Relative paths inherited from the base are therefore joined to the
  **child's** root. **(verified:** a child in `src/examples/b` extending the
  root inherited `libs = [".turnkey/soldeps/vendor"]` and root-relative
  remappings verbatim, then failed to find
  `src/examples/b/.turnkey/soldeps/vendor/...`.)

### Remappings: sources and precedence

- **Where remappings come from.** `RemappingsProvider` collects them in this
  order, and the first entry for a given `context:prefix` wins
  [\[f-remap-get\]][f-remap-get], [\[f-remap-inner\]][f-remap-inner]:
  1. `FOUNDRY_REMAPPINGS` / `DAPP_REMAPPINGS`;
  2. `<root>/remappings.txt`;
  3. `remappings` in `foundry.toml`;
  4. auto-detected remappings from `libs`, if `auto_detect_remappings` is
     on (the default).

  **(verified:** with the same prefix in all three explicit sources, env
  beat `remappings.txt`, which beat the toml.) The doc comment on
  `get_remappings` lists the opposite order; the code and the experiment
  agree with each other.
- **What auto-detection does.**
  - It runs `Remapping::find_many` over each `libs` dir.
  - It also loads the `foundry.toml` of each *direct* child of a `libs` dir
    and takes that file's remappings [\[f-remap-nested\]][f-remap-nested].
  - Auto-detected entries never override an explicit entry with an
    overlapping prefix [\[f-remap-push\]][f-remap-push].
  - Remapping a name equal to the project's own `src`/`test`/`script` dir is
    ignored [\[f-remap-push\]][f-remap-push].
- **`remappings.txt` is only read at the config root**
  [\[f-remap-get\]][f-remap-get]. A per-package `foundry.toml` does not see
  a repo-root `remappings.txt`.
- **Auto-detection against the soldeps cell (verified).** With
  `libs = [".turnkey/soldeps/vendor"]`:
  - `forge-std/` came out right (`vendor/forge-std/src/`, found through
    forge-std's own `foundry.toml`, as an absolute `/nix/store` path).
  - `@openzeppelin/` came out as `vendor/@openzeppelin/contracts/`.
    OpenZeppelin's imports are `@openzeppelin/contracts/…`, so resolution
    produced `…/contracts/contracts/…` and failed.
  - With `auto_detect_remappings = false` and an explicit list derived from
    the cell's `remappings.txt` (plus the forge-std `src/` fix), both
    examples built and passed.
- The book documents `auto_detect_remappings` and the `[<context>:]` syntax
  [\[book-solc\]][book-solc].

### `[dependencies]`, `forge install` and Soldeer

- **`forge install` does not use `[dependencies]`.** It manages git
  submodules under `lib/`, recorded in `.gitmodules` plus `foundry.lock`
  [\[book-deps\]][book-deps].
- **`[dependencies]` is Soldeer's table.**
  - `Config.dependencies` is `Option<SoldeerDependencyConfig>`
    [\[f-deps-field\]][f-deps-field].
  - Soldeer uses `foundry.toml` as its config whenever it has a
    `[dependencies]` table [\[sd-config-path\]][sd-config-path].
  - Soldeer reads a **string** value as a registry version requirement, and
    only a **table** (`{ version, git, rev | branch | tag }` or
    `{ version, url }`) as a git or URL source
    [\[sd-parse\]][sd-parse], [\[f-soldeer\]][f-soldeer].
  - So turnkey's `forge-std = "https://github.com/foundry-rs/forge-std@v1.8.0"`
    is, to `forge soldeer`, a request for registry package `forge-std` at
    version `https://…@v1.8.0` *(inferred; not run, since it needs the
    network)*.
- **forge build/test ignore the table** **(verified:** `forge config` echoes
  it back, and build and test are unaffected).
- The book's Soldeer page shows `[dependencies]` in `soldeer.toml`
  [\[book-soldeer\]][book-soldeer]. The source accepts either file.

## 2. solc import resolution, and what forge passes

- **Base path and include paths.** solc looks up a source unit name under
  the base path, then under each include path in order. The docs recommend
  setting the base path to the project root and using include paths for
  library directories [\[solc-base\]][solc-base].
- **Relative imports.** An import starting with `./` or `../` resolves
  against the importing file's *source unit name*, not the filesystem
  [\[solc-rel\]][solc-rel]. So `import "../src/Counter.sol"` depends on
  where the test's source unit sits.
- **Remappings.**
  - They are `context:prefix=target`, matched against source unit names.
  - When several match, the longest context wins, then the longest prefix
    [\[solc-remap\]][solc-remap].
- **Allowed paths.**
  - Outside Standard JSON, solc may read from the directories of the input
    files, from remapping targets, and from the base and include paths.
  - In Standard JSON mode it may read only from the base and include paths.
  - Anything else needs `--allow-paths` [\[solc-allow\]][solc-allow].
- **What forge passes.** forge runs solc in Standard JSON mode with:
  - `--base-path <root>` and `cwd = <root>`;
  - `--include-path` for each configured include path, excluding the root;
  - `--allow-paths` = root ∪ `libs` ∪ `allow_paths`;
  - remappings in the JSON settings.

  [\[fc-solc-cli\]][fc-solc-cli], [\[fc-project\]][fc-project],
  [\[f-paths\]][f-paths]. *(inferred)* With one repo root, every source
  unit name is repo-relative (`src/examples/a/test/A.t.sol`), and relative
  imports keep working because each file keeps its real place in the tree.
- **What turnkey's Buck2 rule does.** `solidity_library` does not pass
  `--base-path`. It runs `solc` from the Buck2 project root with the source
  paths Buck2 gives it and absolute remapping targets. Remapping targets are
  allowed paths in CLI mode, so no `--allow-paths` is needed there
  [\[solc-allow\]][solc-allow]. See section 3 for the details.

## 3. Turnkey today: every reader of `foundry.toml` and every remapping builder

Paths are relative to the repo root, at `main` (`1f0fca95`).

| Where | What it does with `foundry.toml` / remappings | What a single root changes |
|---|---|---|
| `foundry.toml` (root) | `src = test = "examples"` (a stale path: nothing lives at `examples/`), `libs = ["lib"]` (does not exist), `[dependencies] forge-std = "…@v1.8.0"`. Native `forge build` at the root: "Nothing to compile" **(verified)**. | Becomes the only file. Its `src`/`libs`/remappings need real values (section 5). |
| `src/examples/solidity-hello{,-deps}/foundry.toml` | Per-example roots with `src`/`test`/`libs = ["lib"]`, `solc_version = "0.8.33"`, `fuzz_runs` (forge 1.5.1: "unknown `fuzz_runs` config", **verified**). `-deps` repeats `[dependencies]`. Native `forge build` in `-deps` fails on `@openzeppelin/…` (no `lib/`, no remapping) **(verified)**. | Deleted, or turned into thin files that restate their paths (candidate C). |
| `src/cmd/soldeps-gen/src/main.rs:40-42` | `--foundry` path, default `foundry.toml`, relative to cwd. | Unchanged if the file stays at the root. |
| `…/main.rs:85-105` | Deserialises the top-level `[dependencies]` as `BTreeMap<String, String>`. Also parses `profile.*.remappings/libs`, but those are dead code. | A Soldeer-style table value would fail to deserialise *(inferred from the type)*. Profiles and `src` are ignored, so extra profiles are harmless. |
| `…/main.rs:154-183` | `name = "repo@rev"` → git package with remapping `name/=lib/name/src/`. | This remapping target is thrown away by the cell (below). |
| `…/main.rs:196` | npm packages → `name/=node_modules/name/`. | Same. |
| `…/main.rs:444-465` | Reads only `[dependencies]`. A missing file is not an error. | Unchanged. |
| `nix/buck2/options.nix:455-461` | `solidity.foundryTomlFile`, default `"foundry.toml"`. | Unchanged. |
| `nix/buck2/languages.nix:269-291` | The sync rule: sources = foundry.toml + package.json + pnpm lock. It runs `soldeps-gen --foundry … --prefetch`. | Unchanged, unless git deps move out of `foundry.toml` (decision D below). |
| `flake.nix:497-509` | Self-check: the solidity rule both reads and watches `foundryTomlFile`. | Same as above. |
| `nix/lib/deps-cell/adapters/solidity.nix:160-178` | Writes `remappings.json`. **Nothing reads it** (grep finds no reader). | Dead output. Drop it or use it. |
| `…/solidity.nix:213-226` | Writes the cell's `remappings.txt` as `<prefix>=vendor/<name>/`. It keeps only the prefix of soldeps-gen's remapping and **drops its target**, so `forge-std/` loses `src/`. `solc "forge-std/=<cell>/vendor/forge-std/" …Test.sol` → "Source …/vendor/forge-std/Test.sol not found" **(verified)**. No example imports forge-std today, so nothing notices. | This is the natural single source of remappings for both Buck2 and native forge, once it honours the target path. |
| `…/solidity.nix:186-199` | `soldeps//:bundle` = `remappings.txt` + every `vendor/<name>`. | Unchanged. |
| `nix/buck2/prelude-extensions/solidity/solidity.bzl:62-78` | The macros default `soldeps = "soldeps//:bundle"` when a `soldeps` cell exists. | Unchanged. |
| `…/solidity_library.bzl:110-122` | Reads the bundle's `remappings.txt` and turns each target into an absolute path in a dereferenced copy of the cell. | Unchanged. It never reads `foundry.toml`. |
| `…/solidity_library.bzl:130-145` | `solc` with no `--base-path`, sources as Buck2 paths, `remappings` attr added (lines 169-175). | Unchanged. |
| `…/solidity_test.bzl:131-145` | Flattens the files into a temp project: test srcs into `test/`, dep `.sol` files into `src/`, dep directories symlinked into `lib/`. | *(inferred)* It only works because each example uses `src/` + `test/` with `../src/X.sol` imports. Name collisions or other relative layouts would break. Independent of the root config. |
| `…/solidity_test.bzl:147-180` | Builds `remappings.txt` from the cell (absolute) plus the `remappings` attr, and writes **its own** `foundry.toml` (`src`/`test`/`lib`/`out`). | The repo's `foundry.toml` is never consulted, so root compiler settings (optimizer, `evm_version`, …) do not reach Buck2 tests. |
| `…/toolchain.bzl:49,94-97`, `providers.bzl:18` | `soldeps_path` toolchain attribute. No rule reads it. | Dead. |
| `src/go/pkg/mapper/mapper.go:416-444` | Solidity mapping is enabled only if `<projectRoot>/foundry.toml` (or a hardhat config) exists. It reads package names from `solidity-deps.toml`. | Needs the root file to exist, which a single root guarantees. |
| `src/go/pkg/mapper/mapper.go:1112-1148` | Maps an import's first path segment (`@scope/pkg` or `pkg`) to `soldeps//:<pkg>`. This bakes in "remapping prefix = package name". | *(inferred)* Any remapping whose prefix differs from the package name (e.g. `@oz/=…`) would be unmapped. |
| `src/rust/deps-extract/src/languages/solidity.rs:134-157` | Classifies `./`/`../` imports as internal and everything else as external. | Unchanged. |
| `src/cmd/check-foundry-config/__main__.py:85-100,103-149`; hook in `nix/devenv/turnkey/git-hooks.nix:82-90` | Finds every `foundry.toml` (skipping dot-dirs). Checks `solc_version` against `solc --version`, and that per-package `[dependencies]` are a subset of the root's. Runs only when a `foundry.toml` changes. | The subset check becomes vacuous. The solc check keeps value. *(inferred)* The examples' `0.8.33` pin would fail it against the 0.8.34 toolchain the next time the hook runs. |
| `docs/user-manual/src/languages/solidity.md:28,114-131` | Documents the per-project `foundry.toml` and `[dependencies]` support. | Needs rewriting with the decision. |

## 4. How other monorepos do it

- **Foundry book.** For monorepos: "use a root `foundry.toml` or per-project
  configs". It shows a shared root `lib/` reached through
  `libs = ["lib", "../lib"]` from each package's file
  [\[book-layout\]][book-layout]. That is per-package roots, not one root.
- **Sablier `evm-monorepo`** (`19344450`):
  - A shared **`foundry.base.toml`**, deliberately not named `foundry.toml`
    so it never becomes a project root. It sets `src = "src"`,
    `test = "tests"`, `auto_detect_remappings = false` and
    `allow_paths = ["../"]` [\[sablier-base\]][sablier-base].
  - Each package (`lockup/`, `flow/`, …) has a thin `foundry.toml` that
    repeats `extends = "../foundry.base.toml"` in **every** profile, plus its
    own `remappings.txt` pointing at its own `node_modules/`
    [\[sablier-lockup\]][sablier-lockup],
    [\[sablier-remap\]][sablier-remap].
  - This is the shape of candidate C, and it matches the path re-anchoring
    finding: the base carries only package-relative paths.
- **Optimism** (`bf4fc514`): one self-contained `foundry.toml` per Foundry
  package (`packages/contracts-bedrock/`, test fixtures elsewhere). Explicit
  `remappings` into a package-local `lib/`
  [\[op-bedrock\]][op-bedrock]. It uses `compilation_restrictions` and
  `additional_compiler_profiles` for per-file optimizer settings inside one
  config. Those keys exist in 1.5.1 [\[f-restrict\]][f-restrict].
- **Nothing found that runs many packages from a single root file** in these
  sources. The pattern appears to be one root per package, with shared
  settings through `extends` or copy *(inferred from a small sample; not a
  survey)*.

## 5. Candidate root layouts

All three assume the same things:

- the soldeps cell stays the one place dependencies live on disk
  (`.turnkey/soldeps/vendor/<name>`);
- `auto_detect_remappings = false`;
- one remapping list, generated from `solidity-deps.toml` and feeding both
  Buck2 (the cell's `remappings.txt`) and native forge.

### A. One root, repo-wide paths

```toml
[profile.default]
src = "src"
test = "src"
script = "src"            # or leave unset; see open question 6
libs = [".turnkey/soldeps/vendor"]
out = "out"
auto_detect_remappings = false
solc_version = "0.8.34"
# remappings: generated by tk sync into <root>/remappings.txt
```

Verified end to end in a scratch copy: both example suites pass from the root
and from a subdirectory.

- **Pros:**
  - Literally one file, matching the Go/Rust/Python rule.
  - Relative imports and source unit names stay repo-relative.
  - No per-package files to generate.
  - The pre-commit subset check can be deleted.
- **Cons:**
  - Every `forge` run compiles all Solidity under `src/`.
  - Running `forge test` in an example runs everything, unless you pass
    `--match-path 'src/examples/x/**'`.
  - `out/` and `cache/` land at the repo root and must be git-ignored.
  - One optimizer setting for all packages, unless you use
    `compilation_restrictions`.

### B. A + one profile per package

As A, plus a generated profile per package:

```toml
[profile.solidity-hello]
src = "src/examples/solidity-hello/src"
test = "src/examples/solidity-hello/test"
out = "out/solidity-hello"
```

- **Pros:**
  - Scoped runs through `FOUNDRY_PROFILE=solidity-hello forge test`, from
    anywhere **(verified)**.
  - Paths stay root-relative, so there is no re-anchoring.
  - Still one file.
- **Cons:**
  - Scoping by env var is less discoverable than `cd`.
  - N profiles to keep in sync. They could be generated by `tk sync`, but
    then the file is partly generated.
  - `soldeps-gen` and the check script must keep ignoring profiles (they do
    today).

### C. Root base + thin per-package files (the Sablier shape)

The root file holds the shared settings. Each package has a
`foundry.toml` with `extends = "../../../foundry.toml"` in every profile,
**plus** its own `libs = ["../../../.turnkey/soldeps/vendor"]` and
remappings (or a package `remappings.txt`), because inherited paths
re-anchor.

- **Pros:**
  - `cd src/examples/x && forge test` is naturally scoped, with a
    per-package `out/`.
  - This is the pattern the one real monorepo found uses.
- **Cons:**
  - It is not one file.
  - The thin files carry path-bearing boilerplate that must be generated
    (by `tk sync`) or hand-kept.
  - It keeps the check script's reason to exist.
  - If the base is named `foundry.toml`, running at the root still builds
    everything (A for free). If it is named `foundry.base.toml`, as Sablier
    does, the root has no project.
  - The `extends` path depth is brittle when packages move.

### Orthogonal decision D: where git dependencies are declared

1. **Keep turnkey's `[dependencies] name = "url@tag"`.** Zero churn, but it
   collides with Soldeer's schema: `forge soldeer install/update` would
   misread it.
2. **Use Soldeer's table form**, e.g.
   `forge-std = { version = "1.8.0", git = "https://github.com/foundry-rs/forge-std.git", tag = "v1.8.0" }`.
   `forge soldeer` can read it, but `soldeps-gen` must parse tables and map
   `rev`/`branch`/`tag`.
3. **Move git deps out of `foundry.toml`.** Options are a turnkey-owned
   table, or git specifiers in `package.json`, which pnpm already locks.
   `foundry.toml` then becomes pure forge config, and the sync rule's
   sources change.

## Open questions for the grilling session

1. Must `forge test` inside an example directory stay scoped to that
   example (which favours C), or is root-only plus `--match-path` or a
   profile enough (A or B)?
2. Are native `forge` runs a supported workflow at all, or is Buck2
   (`tk test`) the only supported path? If Buck2 only, A with no per-package
   story is sufficient.
3. Should the root remapping list be generated by `tk sync`, and into which
   file? `remappings.txt` has higher precedence and keeps `foundry.toml`
   hand-written. The `remappings` key keeps it to one file.
4. Should soldeps-gen's per-package `remapping` (including its target path)
   become authoritative in the cell's `remappings.txt`? That fixes the
   forge-std bug. The alternative is that the cell stops pretending and
   soldeps-gen emits prefixes only.
5. D: which home for git dependencies? And do we care about `forge soldeer`
   compatibility?
6. Should `script` point anywhere? With `script = "src"`, `forge build`
   compiles scripts too, but no scripts exist yet.
7. Should root compiler settings (optimizer, `evm_version`, `solc_version`)
   also drive Buck2's `solidity_library`/`solidity_test`, which ignore
   `foundry.toml` today? If so, which side generates which?
8. Should `solidity_test` stop flattening into `src/` and `test/` and keep
   repo-relative paths, as forge does from a single root? This is separate
   from the layout, but the same "one source of truth" question.
9. What should `check-foundry-config` check under a single root: solc
   version only, or also "no nested `foundry.toml`"?

## Could not verify

- `forge soldeer` actually misreading the current `[dependencies]` string
  form. The source reading is unambiguous, but it was not run, because it
  needs the network.
- Which forge version introduced `extends`. It is present in 1.5.1, and a
  Sablier comment mentions forge 1.8, so the upstream toolchain has moved
  on since the pinned version.
- Whether the Buck2 test runner sets `HOME`, which decides whether
  `~/.foundry/foundry.toml` leaks into `solidity_test`.
- Behaviour on Linux. All experiments ran on aarch64-darwin.

## Sources

[f-root]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/utils.rs#L34-L72
[f-cli-root]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/cli/src/opts/build/core.rs#L183-L199
[f-figment]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L798-L873
[f-fields]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L188-L228
[f-paths]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L1313-L1330
[f-profile]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L1945-L1962
[f-merge-toml]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L2205-L2253
[f-extend]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/extend.rs#L1-L60
[f-ext-read]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/providers/ext.rs#L82-L200
[f-remap-get]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/providers/remappings.rs#L143-L245
[f-remap-inner]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/providers/remappings.rs#L61-L66
[f-remap-push]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/providers/remappings.rs#L67-L115
[f-remap-nested]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/providers/remappings.rs#L246-L308
[f-deps-field]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L515-L520
[f-soldeer]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/soldeer.rs
[f-restrict]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/config/src/lib.rs#L545-L551
[f-test-sources]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/forge/src/cmd/test/mod.rs#L210-L229
[f-test-filter]: https://github.com/foundry-rs/foundry/blob/b0a9dd9ceda36f63e2326ce530c10e6916f4b8a2/crates/forge/src/cmd/test/filter.rs#L216-L222
[fc-inputs]: https://github.com/foundry-rs/compilers/blob/7bb89c3dcd19b7f2fcc5f849fd614af382965e3c/crates/compilers/src/config.rs#L603-L615
[fc-project]: https://github.com/foundry-rs/compilers/blob/7bb89c3dcd19b7f2fcc5f849fd614af382965e3c/crates/compilers/src/compile/project.rs#L514-L518
[fc-solc-cli]: https://github.com/foundry-rs/compilers/blob/7bb89c3dcd19b7f2fcc5f849fd614af382965e3c/crates/compilers/src/compilers/solc/compiler.rs#L492-L512
[sd-config-path]: https://github.com/mario-eth/soldeer/blob/ccbc65322391b5a263bb006e803182f762c29b86/crates/core/src/config.rs#L714-L740
[sd-parse]: https://github.com/mario-eth/soldeer/blob/ccbc65322391b5a263bb006e803182f762c29b86/crates/core/src/config.rs#L923-L990
[book-overview]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/config/reference/overview.mdx#L5-L45
[book-project]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/config/reference/project.mdx#L5-L57
[book-profiles]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/config/profiles.mdx#L84-L100
[book-solc]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/config/reference/solidity-compiler.mdx#L16-L75
[book-layout]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/projects/layout.mdx#L84-L126
[book-deps]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/projects/dependencies.mdx
[book-soldeer]: https://github.com/foundry-rs/book/blob/415e49165f252a8226e2983286b6748882f6e11a/src/pages/projects/soldeer.mdx#L36-L66
[solc-base]: https://github.com/argotorg/solidity/blob/v0.8.34/docs/path-resolution.rst#L294-L341
[solc-rel]: https://github.com/argotorg/solidity/blob/v0.8.34/docs/path-resolution.rst#L222-L245
[solc-allow]: https://github.com/argotorg/solidity/blob/v0.8.34/docs/path-resolution.rst#L414-L436
[solc-remap]: https://github.com/argotorg/solidity/blob/v0.8.34/docs/path-resolution.rst#L485-L634
[sablier-base]: https://github.com/sablier-labs/evm-monorepo/blob/19344450e9182db61e6cb971d3c1bc6ae0cdc70e/foundry.base.toml
[sablier-lockup]: https://github.com/sablier-labs/evm-monorepo/blob/19344450e9182db61e6cb971d3c1bc6ae0cdc70e/lockup/foundry.toml
[sablier-remap]: https://github.com/sablier-labs/evm-monorepo/blob/19344450e9182db61e6cb971d3c1bc6ae0cdc70e/lockup/remappings.txt
[op-bedrock]: https://github.com/ethereum-optimism/optimism/blob/bf4fc514f8ad9e80cacacf47070029a3248538c1/packages/contracts-bedrock/foundry.toml

### Revisions read

All read on 2026-09-26.

- `foundry-rs/foundry` at tag `v1.5.1` (commit `b0a9dd9c`), the version the
  devshell's `forge` reports.
- `foundry-rs/compilers` at `v0.19.6` (`7bb89c3d`) and `mario-eth/soldeer`
  at `v0.9.0` (`ccbc6532`), as resolved by foundry v1.5.1's `Cargo.lock`.
- `foundry-rs/book` at `415e4916` (main, 2026-09-26).
- Solidity docs at tag `v0.8.34`, `docs/path-resolution.rst`, also rendered
  at <https://docs.soliditylang.org/en/v0.8.34/path-resolution.html>.
- `sablier-labs/evm-monorepo` at `19344450` and
  `ethereum-optimism/optimism` at `bf4fc514` (default branches on that
  date).
- Experiments used `forge 1.5.1-v1.5.1` (`b0a9dd9c`) and
  `solc 0.8.34+commit.80d5c536` from `solidity-toolchain` v2, on
  aarch64-darwin.
