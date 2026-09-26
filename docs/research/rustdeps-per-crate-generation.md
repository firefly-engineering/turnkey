# Where does `rustdeps` cell generation spend its time, and should it be split per crate?

Research for [Investigate per-dependency BUCK generation for Rust](https://github.com/firefly-engineering/turnkey/issues/35),
part of map [Rust dependency cell: correct feature resolution and cheaper BUCK
generation](https://github.com/firefly-engineering/turnkey/issues/82).
Gathered on 2026-09-26. The measurements come from turnkey at `main`
(`1f0fca95`, "fix(rust): resolve vendored crate features and deps the way Cargo
does"). The prior art comes from upstream source pinned to the commits listed in §4.
All URLs were accessed 2026-09-26.

**Measured** marks a number taken on this machine. *(inferred)* marks a
conclusion drawn from measurements or sources rather than observed directly.
*(unverified)* marks something no measurement or source here settles.

The ticket asks three things:

1. Where does `rustdeps` cell generation spend its time?
2. How do crate2nix, cargo2nix, nixpkgs `buildRustCrate`/`importCargoLock`,
   crane and reindeer split per-crate work while keeping feature unification
   global?
3. Can the output of `src/python/cargo/turnkey/cargo/features.py` feed per-crate
   derivations, so that a one-crate lockfile change only invalidates the
   affected crates?

## TL;DR

- **The ticket's premise is half wrong.** Fetching is already per crate.
  - Each crate is its own `fetchzip` fixed-output derivation, plus its own
    `dep-rust-<name>-<version>` derivation that applies build-script fixups.
  - Only the **merge** is monolithic: copy, feature unification and BUCK
    generation run in one `rustdeps-cell` derivation.
  - A one-crate bump rebuilt exactly **2 derivations**: the crate's
    `dep-rust-*` and the cell.
- **The merge takes ~57 s. Almost none of that is real work.**
  - Feature unification takes **0.15 s**.
  - Generating all 315 `rules.star` files in one Python process takes
    **0.25 s**, byte-identical to the cell's output.
  - The time goes to:
    - **~20–28 s spawning processes.** `gen-rust-buck` runs **594** times
      (315 crate directories plus 279 unversioned symlinks that point back at
      them), and each run is a bash wrapper, a fresh Python and JSON parsing.
    - **~8 s** copying 549 MB and 16,425 files into `$out`.
    - **~21 s** of Nix registering that output. This machine has
      `auto-optimise-store = true`.
- **A one-crate bump took 76.5 s end to end** (`anyhow` 1.0.100 → 1.0.101).
  Only **2 of 315** `rules.star` files changed: `anyhow`'s own and
  `prost-derive`'s, which names `anyhow@1.0.101`. Only 2 entries in the
  unified feature map changed, both the renamed `anyhow` key.
- **Prior art:**
  - Every tool that has per-crate units (crate2nix, cargo2nix, reindeer)
    unifies features in **one global step**. It then hands each crate its
    slice. crate2nix does this in Nix at evaluation time; the others do it at
    generation time.
  - Tools that don't do that (crane, `buildRustPackage`) leave unification to
    cargo inside one big build.
  - Nobody unifies per crate.
- **Feasibility:** yes. `compute_unified_features` already returns a per-crate
  map. But `gen-rust-buck` also takes the **global** `available_crates` list,
  which changes whenever any crate is added, removed or bumped. So the global
  step must also resolve each crate's dependencies to `name@version` targets,
  as crate2nix's JSON mode and `Cargo.lock` do.
- **Recommendation:** don't split into per-crate Nix derivations to save
  generation time.
  - The measured work per crate (~1 ms) is ~350× smaller than the measured
    overhead of one Nix derivation (~0.35 s).
  - Instead:
    - **(1)** generate all BUCK files in a single process. This saves an
      estimated 20–48 s per cell build.
    - **(2)** stop copying 549 MB into the cell.
  - The question that could still justify a per-crate split is **not
    generation time**: see §3.3. The whole cell enters buck2's action keys as
    one store path. So *(inferred)* any lockfile change probably makes buck2
    rebuild **every** vendored crate. That is the likely dominant cost of a
    one-crate change, and it is unmeasured.

## 1. How the cell is built today

- **The flake exposes it as `packages.<system>.rustdeps-cell`.**
  [`nix/flake-parts/turnkey/default.nix` L376](../../nix/flake-parts/turnkey/default.nix#L376)
  maps every language cell to a `<cell>-cell` package. Evaluating it needs
  `--impure`, because the `ring` fixup reads `builtins.currentSystem`
  ([`nix/lib/deps-cell/fixups/rust/ring.nix` L73](../../nix/lib/deps-cell/fixups/rust/ring.nix#L73)).
  The repo's `.envrc` already uses `--no-pure-eval`.
- **Per-crate work is already its own derivation.**
  - `languages.nix` calls `mkRustDepsCell`
    ([`nix/buck2/languages.nix` L99](../../nix/buck2/languages.nix#L99)).
  - For each of the 315 `rust-deps.toml` entries, that builds a
    `dep-rust-<name>-<version>` `runCommand`. It copies a `fetchzip` of the
    crate and runs its build-script fixup
    ([`nix/lib/deps-cell/adapters/rust.nix` L41-L69](../../nix/lib/deps-cell/adapters/rust.nix#L41-L69)).
  - The cell `.drv` has **321 input derivations**: 315 crates plus tools.
    **Measured** with `nix derivation show`.
- **The merge is one `runCommand "rustdeps-cell"`**
  ([`nix/lib/deps-cell/default.nix` L95-L110](../../nix/lib/deps-cell/default.nix#L95-L110)).
  In order, it:
  - `cp -r`s every crate into `$out/vendor/<name>@<version>`;
  - adds an unversioned symlink per crate name;
  - applies user patches;
  - runs `compute-unified-features` once over the whole `vendor/`
    ([`rust.nix` L170](../../nix/lib/deps-cell/adapters/rust.nix#L170));
  - runs a shell loop `for dir in "$out/vendor"/*` that calls `gen-rust-buck`
    once per directory
    ([`rust.nix` L180-L191](../../nix/lib/deps-cell/adapters/rust.nix#L180-L191)).
- **The loop runs twice per crate.** `[ -d "$dir" ]` is true for the
  unversioned symlinks as well. So 279 crates are generated a second time
  through their symlink, with identical arguments, overwriting the same file.
  **Measured**: 594 entries in `vendor/`, 279 of them symlinks.
- **Each `gen-rust-buck` call gets five JSON arguments inlined into the build
  script:**
  - every crate name;
  - the fixup crate names;
  - the full unified-features map;
  - the rustc flags registry;
  - the native-library map.

  Only the crate's own `Cargo.toml` is read from disk
  ([`src/cmd/gen-rust-buck/__main__.py` L36-L41](../../src/cmd/gen-rust-buck/__main__.py#L36-L41)).
- **The output is large.** The cell is 549 MB in 16,425 files. The largest
  entries are Windows import-library crates that never build on darwin or
  Linux:
  - `winapi-x86_64-pc-windows-gnu`: 54 MB
  - `winapi-i686-pc-windows-gnu`: 52 MB
  - `windows-sys@0.48.0`: 30 MB

  Each crate's source therefore exists three times in the store: the
  `fetchzip` output, the `dep-rust-*` copy and the cell copy *(inferred from
  the derivation structure)*.

## 2. Measurements

**Setup:**
- Machine: Apple M5 Max, 18 cores, Nix 2.34.7.
- Nix settings: `sandbox = false`, `auto-optimise-store = true`,
  `max-jobs = 18`.
- Lockfile: turnkey's own `rust-deps.toml`, with 315 crates.
- All Nix commands evaluated the narrow attribute
  `.#packages.aarch64-darwin.rustdeps-cell`. Logs are in `/tmp/rd-*.txt`.

### 2.1 Breakdown

| Phase | How measured | Time |
|---|---|---|
| Evaluate the cell `drvPath`, source unchanged | `nix eval --impure --option eval-cache false --raw …drvPath`, 3 runs | 0.97–1.29 s |
| Evaluate plus instantiate after the scratch edit, up to "these 2 derivations will be built" | timestamps on `nix build -L` | 6.9 s |
| One per-crate `dep-rust-anyhow-1.0.101` derivation, wall time including registration | timestamps on `nix build -L` | ~0.35 s |
| **Whole merge derivation**, inside Nix | `nix build --rebuild` of the cell `.drv`, 2 runs | **57.0 s, 58.0 s** |
| ↳ copy crates plus symlinks, until "Computing unified features" | timestamped build log | 8.4 s |
| ↳ `compute-unified-features` | timestamped build log | 0.2 s |
| ↳ BUCK loop plus output registration | timestamped build log | 49.4 s |
| Same build script, **outside** Nix (`$out=/tmp/rd-out`), timestamps between phases | instrumented copy of the `.drv`'s `buildCommand` | 27.9 s total |
| ↳ copy | same | 7.4 s |
| ↳ symlinks | same | 0.7 s |
| ↳ `compute-unified-features` | same, re-run alone: 0.15 s | 0.19 s |
| ↳ BUCK loop, 594 calls | same, and 2 more runs: 19.7 s, 19.7 s | 19.6 s |
| BUCK loop skipping symlinked dirs (315 calls) | same loop with `[ ! -L "$dir" ]`, 2 runs | 10.5 s, 10.0 s |
| **All 315 `rules.star` in one Python process** | `runpy` of `gen-rust-buck/__main__.py`, calling `main()` per crate | **0.25 s** (0.8 ms/crate) plus 0.04 s import. Output matches the cell byte for byte: 0 mismatches out of 315 |
| Nix registering a 549 MB / 16k-file output | probe `runCommand` that only does `cp -r` of the cell into `$out`: "copied" at 7.9 s, build returned at 28.7 s | **~21 s** after the builder exited |
| `nix hash path` of the same tree | 2 runs | 0.53 s |
| **One-crate bump, end to end** (`anyhow` 1.0.100 → 1.0.101 in a scratch `rust-deps.toml`, hash already known) | `nix build --impure --no-link -L .#…rustdeps-cell` | **76.5 s** |

Reading the table:

- **The BUCK phase is process overhead.**
  - One process does all 315 crates in 0.25 s.
  - 315 processes take 10 s, and 594 take 20 s outside Nix, so each call costs
    ~32 ms.
  - Inside Nix the same loop sits in the 49 s bucket together with
    registration. Subtracting the ~21 s probe figure leaves ~28 s for the loop
    *(inferred)*, slower than the 20 s measured outside Nix.
- **Nix output registration is the second cost.** It is not NAR hashing,
  which takes 0.5 s for this tree. It is plausibly `auto-optimise-store`
  hard-linking 16k files *(inferred; not isolated)*.
- **Feature unification is negligible**: 0.15–0.2 s.
- **Evaluation is cheap today**: ~1 s. It is not the bottleneck.

### 2.2 What a one-crate change actually changes

After the `anyhow` bump, comparing the old and new cell outputs (measured,
`diff -rq`):

- Only 2 derivations were rebuilt: `dep-rust-anyhow-1.0.101` and the cell.
- Only 2 `rules.star` files changed:
  - `vendor/anyhow@1.0.101` (a new directory);
  - `vendor/prost-derive@0.14.4`, whose dependency label moved from
    `rustdeps//vendor/anyhow@1.0.100:anyhow` to `…@1.0.101:anyhow`.

  Every other file outside `vendor/anyhow*` is byte-identical.
- In the unified feature map, only the two `anyhow` keys differ: the
  `@1.0.100` entry disappeared and `@1.0.101` appeared, both with `["std"]`.
  No other crate's features changed.

So a perfectly incremental scheme would have done ~1 ms of generation work
plus one fetch. Today's scheme does ~57 s.

## 3. Could `features.py` feed per-crate derivations?

### 3.1 What the global step needs and produces

- **What it reads.** `compute_unified_features(vendor_dir, overrides,
  requested)` reads every vendored `Cargo.toml` through
  `load_vendored_crates(vendor_dir)`
  ([`features.py` L152, L261-L296](../../src/python/cargo/turnkey/cargo/features.py#L152)).
  It needs the **manifests** of all crates, not their sources.
- **What it returns.** A JSON map `"name@version" -> [features]`.
  - That is already the per-crate slice a per-crate derivation would take.
  - Measured: 315 keys, 14.9 KB.
- **The same algorithm run on the bumped tree** changed only the `anyhow`
  entries (§2.2). The unification that
  [Rust feature unifier enables default features nobody requested](https://github.com/firefly-engineering/turnkey/issues/57)
  and [gen-rust-buck resolves renamed optional deps by name](https://github.com/firefly-engineering/turnkey/issues/38)
  made correct is untouched by this. It stays one global pass.

### 3.2 The blocker: `available_crates` is global too

`gen-rust-buck` resolves each dependency against **every** vendored
`name@version`:

- `resolve_dep` → `find_matching_version` scans `available_crates` for the
  highest version matching the requirement
  ([`src/python/buck/turnkey/buck/generator.py` L76-L117](../../src/python/buck/turnkey/buck/generator.py#L76-L117)).
- `filter_features_for_availability` also consults it
  ([`gen-rust-buck/__main__.py` L69-L73](../../src/cmd/gen-rust-buck/__main__.py#L69-L73)).

If a per-crate derivation took that list as an input, any crate being added,
removed or bumped would change every crate's derivation. That is the same
invalidation as today.

**Needed for a split** *(inferred)*: the global step must emit, per crate, the
features **and** the resolved dependency labels, meaning each crate's
`resolve_dep` results. Then a per-crate BUCK derivation's inputs are exactly:

- its own source;
- its own features;
- its own resolved dependencies;
- its own fixup, flags and native-library entry.

For the `anyhow` bump, that input set changes for exactly the 2 crates in §2.2.
This is what `Cargo.lock` itself records, and what crate2nix's JSON mode bakes
in (§4.1).

### 3.3 The cell's shape decides what buck2 invalidates

Splitting the derivations only helps buck2 if the cell's *path* stops changing
for unaffected crates. Today it cannot:

- **The cell reaches buck2 as a single absolute symlink.** `.turnkey/rustdeps
  -> /nix/store/<hash>-rustdeps-cell`, set up in `nix/devenv/turnkey/buck2.nix`.
- **buck2 stops at the first absolute symlink** and keys the file as
  `ExternalSymlink(target, rest)`. So the whole cell enters action keys as its
  store path. See
  [remote-execution-and-caching.md §3.2](remote-execution-and-caching.md#32-buck2-at-6507dd15),
  citing [`io/fs.rs` L229-L262](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_common/src/io/fs.rs#L229-L262)
  and [`directory.rs` L626-L665](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/directory.rs#L626-L665).
- **So *(inferred)* any change to `rust-deps.toml` gives a new cell store path.**
  That changes the input key of **every** action that reads a vendored crate:
  all 315 `rust_library` compiles.
  - Measured: the `anyhow` bump moved the cell from `i23ajcg0…` to
    `zwnr4202…`.
  - This is very likely far more expensive than the 57 s merge.
  - *(unverified)*: not measured here, because it needs a full
    `tk build //...` before and after. That was outside this ticket's build
    budget.
- **Only one layout would fix this** *(inferred)*: `.turnkey/rustdeps` becomes
  a real directory whose `vendor/<crate>` entries are absolute symlinks to
  **per-crate** store paths, each holding the source plus `rules.star`. Then
  each crate's key would be its own store path.
  - This depends on buck2 accepting BUCK files and sources reached through
    per-directory external symlinks inside a cell. That is *(unverified)*,
    although today's cell is already read through one.
  - It also interacts with the FUSE-composed cell
    ([`turnkey-composed`](../architecture/fuse-composition-layer.md)), which
    this research did not examine.
  - It moves the "compose" step out of Nix and into the `.turnkey/`
    symlink manager.

## 4. Prior art

Upstream source pinned to these commits:

- crate2nix [`5e1ecfd2`](https://github.com/nix-community/crate2nix/tree/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767)
- cargo2nix [`a709c746`](https://github.com/cargo2nix/cargo2nix/tree/a709c74619e1a2b68ed12bb398e12fbe29d69657)
- nixpkgs [`f258aed3`](https://github.com/NixOS/nixpkgs/tree/f258aed3f7e763a8f4a818647500d5ead72eab4d)
- crane [`73b98051`](https://github.com/ipetkov/crane/tree/73b980519cefc727a5f6cc8e5c0947a2f9be6edd)
- reindeer [`90a692c0`](https://github.com/facebookincubator/reindeer/tree/90a692c0c1272ae08f9e1b1e73015feee51c6b18)

### 4.1 crate2nix

**Default `Cargo.nix` output**

- **Where unification happens: at Nix evaluation time.**
  - The generator runs `cargo metadata --locked`, with `--all-features` by
    default
    ([`main.rs` L465-L474](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/src/main.rs#L465-L474)).
  - It records cargo's own resolve as `resolvedDefaultFeatures`
    ([`Cargo.nix.tera` L299-L300](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/templates/Cargo.nix.tera#L299-L300)).
  - The build path strips that field and **re-resolves in Nix** with
    `mergePackageFeatures`: a whole-graph fixpoint from `rootFeatures ? [
    "default" ]`, folding a `featuresByPackageId` cache
    ([`templates/nix/crate2nix/default.nix` L310-L366, L570-L660](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/templates/nix/crate2nix/default.nix#L570-L660)).
- **Unit of work:** one `buildRustCrate` derivation per package ID
  ([same file L337-L360](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/templates/nix/crate2nix/default.nix#L337-L360)).
- **What one changed crate invalidates:** that crate, its reverse dependencies
  (dependency derivations are inputs), and any crate whose merged feature set
  changed.

**`--format json` output (on master, not in the 0.15.0 changelog)**

- It does the opposite: it takes features from cargo's resolve in Rust, with
  optional dependencies pre-activated. The stated reason is that this
  "eliminates the O(n*m) feature resolution that the Nix template output
  requires at eval time"
  ([`json_output.rs` L1-L6, L285-L300](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/src/json_output.rs#L1-L6)).
- `lib/build-from-json.nix` passes `features = crateInfo.resolvedDefaultFeatures
  or [ ]` straight to `buildRustCrate`
  ([L255](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/lib/build-from-json.nix#L255)).
- **This is the closest analogue to the turnkey design** proposed in #35:
  - one global resolution emitting a JSON slice per crate;
  - one derivation per crate that consumes its slice.

### 4.2 cargo2nix

- **Where unification happens: at generation time**, using cargo's resolver as
  a library.
  - It resolves the workspace once with all features, then once with none.
  - Then it re-resolves once per root feature per member
    ([`main.rs` L224-L292, L393-L466](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/src/main.rs#L393-L466)).
- **What Nix evaluates:** only cheap `lib.optional (<activated_by rootFeatures'>)
  "feat"` selectors
  ([`Cargo.nix.tera` L97-L106](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/templates/Cargo.nix.tera#L97-L106)).
- **Unit of work:** one `mkRustCrate` derivation per crate per profile. Each
  runs `cargo build` on that single crate
  ([`mkcrate.nix` L108, L326](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/overlay/mkcrate.nix#L108)).
- **What one changed crate invalidates:** the same as crate2nix: that crate, its
  reverse dependencies, and crates whose emitted feature list changed.

### 4.3 nixpkgs

**`buildRustCrate`**

- It does no unification. `features` is a plain argument that the caller must
  supply
  ([`build-rust-crate/default.nix` L199-L201, L500-L507](https://github.com/NixOS/nixpkgs/blob/f258aed3f7e763a8f4a818647500d5ead72eab4d/pkgs/build-support/rust/build-rust-crate/default.nix#L500-L507)).
- Each crate's rustc metadata hash folds in its features and every
  dependency's metadata
  ([L511-L526](https://github.com/NixOS/nixpkgs/blob/f258aed3f7e763a8f4a818647500d5ead72eab4d/pkgs/build-support/rust/build-rust-crate/default.nix#L511-L526)).
- So a change invalidates exactly that crate plus its transitive reverse
  dependencies.

**`importCargoLock`**

- It parses `Cargo.lock` at evaluation time.
- It makes one `fetchurl` per crate from the lock checksum, plus one unpack
  derivation per crate.
- All of them are combined into one `cargo-vendor-dir` `runCommand`
  ([`import-cargo-lock.nix` L101-L196, L290](https://github.com/NixOS/nixpkgs/blob/f258aed3f7e763a8f4a818647500d5ead72eab4d/pkgs/build-support/rust/import-cargo-lock.nix#L152-L196)).
- It is structurally the same as turnkey today: fetches per crate, one merge.
- Features are left to cargo inside `buildRustPackage`.

**`fetchCargoVendor`**

- It is **not** per crate. It is one fixed-output derivation over all crates,
  so any lockfile change refetches everything
  ([`fetch-cargo-vendor.nix` L72-L120](https://github.com/NixOS/nixpkgs/blob/f258aed3f7e763a8f4a818647500d5ead72eab4d/pkgs/build-support/rust/fetch-cargo-vendor.nix#L72-L120)).

### 4.4 crane

- **Downloads are per crate.** `downloadCargoPackage` is a `fetchurl` plus an
  unpack
  ([`downloadCargoPackage.nix`](https://github.com/ipetkov/crane/blob/73b980519cefc727a5f6cc8e5c0947a2f9be6edd/lib/downloadCargoPackage.nix#L1-L50)).
- **They are composed by symlinks, not copies:**
  `ln -s ${vendorCrate p} $out/${p.name}-${p.version}` in one
  `runCommandLocal "vendor-registry"`
  ([`vendorCargoRegistries.nix` L55-L70](https://github.com/ipetkov/crane/blob/73b980519cefc727a5f6cc8e5c0947a2f9be6edd/lib/vendorCargoRegistries.nix#L55-L70)).
  - This is the cheap-merge pattern turnkey's merge lacks.
  - A symlink farm's output is tiny, so the ~8 s copy and ~21 s registration
    in §2.1 would not arise *(inferred)*.
- **Features are left to cargo.** `buildDepsOnly` compiles all dependencies in
  one derivation
  ([`buildDepsOnly.nix` L60-L90](https://github.com/ipetkov/crane/blob/73b980519cefc727a5f6cc8e5c0947a2f9be6edd/lib/buildDepsOnly.nix#L60-L90)).
  So one changed crate rebuilds every dependency.

### 4.5 reindeer (Buck2)

- **Where unification happens: one global step at `buckify` time.**
  - `cargo metadata --all-features` supplies the package universe
    ([`cargo.rs` L86-L109](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/cargo.rs#L86-L109)).
  - reindeer's own `FeatureResolver` then unifies features per platform
    ([`index.rs` L160-L178](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/index.rs#L160-L178);
    [MANUAL L286-L291](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/docs/MANUAL.md#L286-L291)).
  - The architecture is the same as turnkey's `features.py` plus
    `gen-rust-buck`: one global pass, per-crate rules.
- **Output:** one `BUCK` file by default, "always completely regenerated"
  ([MANUAL L68-L74](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/docs/MANUAL.md#L68-L74)).
  With `buck.split = true`, it writes one BUCK file per crate
  ([`config.rs` L156-L162](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/config.rs#L156-L162);
  [`buckify.rs` L1709-L1822](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/buckify.rs#L1709-L1822)).
  - reindeer regenerates everything each time and relies on **buck2's own
    action keys** for incrementality *(inferred)*. The split limits which
    files change textually.
- **Sources:**
  - Vendored by default.
  - Without `vendor`, reindeer emits one `http_archive` per crate from the
    lockfile checksum
    ([`buckify.rs` L398-L432](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/buckify.rs#L398-L432)).
  - With `http_archive`, each crate's sources are buck2-managed per crate, so
    a changed crate cannot change another crate's input key. That avoids the
    §3.3 problem entirely *(inferred)*.

### 4.6 Comparison

| Tool | Global unification | Per-crate unit | One crate changes → invalidates | Merge step |
|---|---|---|---|---|
| **turnkey today** | `features.py`, at build time in the cell derivation (0.15 s) | fetch + fixup derivation | one fetch, the whole cell (57 s), then every buck2 rust action *(inferred, §3.3)* | `cp -r` of 549 MB |
| crate2nix (Nix output) | Nix eval, whole-graph fixpoint | `buildRustCrate` derivation | crate, reverse dependencies, feature-changed crates | none |
| crate2nix (JSON output) | generation time, from cargo's resolve | `buildRustCrate` derivation | same | none |
| cargo2nix | generation time, cargo resolver per root feature | `mkRustCrate` derivation | same | none |
| `importCargoLock` | cargo, inside the build | fetch + unpack derivation | one fetch, the vendor directory, the whole cargo build | one `runCommand` |
| crane | cargo, inside `buildDepsOnly` | fetch + unpack derivation | one fetch, symlink farm, all dependencies recompile | symlink farm |
| reindeer | generation time, own resolver per platform | BUCK rule (optionally one file per crate), `http_archive` per crate | BUCK text regenerated; buck2 re-runs changed actions | none (no Nix) |

What they share:

- Every tool keeps unification **global and ahead of the per-crate units**.
- None tries to make unification itself incremental. It is cheap everywhere,
  as it is here (0.15 s).
- Incrementality comes from giving each crate a **stable, content-keyed unit
  that its build consumes**.

## 5. Feasibility for turnkey

- **The features split is easy.**
  - `compute_unified_features` already produces the per-crate map, and it
    runs in 0.15 s.
  - The only change needed is to move dependency resolution (`resolve_dep`)
    into the same global pass, so that no per-crate step sees
    `available_crates` (§3.2).
- **Per-crate BUCK derivations would make generation slower, not faster**
  *(inferred)*:
  - ~0.35 s of Nix overhead per derivation (measured for `dep-rust-anyhow`);
  - × 315 crates, with 18 jobs;
  - ≈ 6–110 s on a cold cell, against 0.25 s for all crates in one process.
  - For a one-crate change, it would save only what (1) and (2) below save
    anyway.
  - The number of `.drv` files would go from ~630 to ~945.
- **The part that matters for buck2 is the cell's store path** (§3.3).
  - Per-crate *store paths* only help if the project-side cell is a directory
    of per-crate symlinks rather than one symlink to a merged cell.
  - That changes how `.turnkey/` is managed. It needs a check against the
    FUSE-composed cell and against remote-execution keys (map #60), because
    each crate becomes its own `SymlinkNode`.

## 6. Recommendation (for grilling)

**Keep one cell derivation. Remove the two measured costs:**

1. **Generate all BUCK files in one process.**
   - Add a batch mode to `gen-rust-buck`, or fold it into
     `compute-unified-features`: read the arguments once and loop over crates
     in-process.
   - Measured: 0.25 s instead of 19.7 s outside Nix, and instead of an
     estimated ~28 s inside Nix.
   - It also removes the doubled loop over symlinks.
   - Low risk: the output was byte-identical for all 315 crates.
2. **Stop copying crate sources into the cell.**
   - Symlink `vendor/<name>@<version>` to the `dep-rust-*` store paths, as
     crane does.
   - Write only `rules.star` files, symlinks and `.buckconfig` into `$out`.
   - This targets the ~8 s copy and the ~21 s registration.
   - *(unverified)*:
     - buck2 must accept a second level of absolute symlinks inside a cell
       that is already reached through one;
     - user patches (applied with `patch -p1` into `vendor/`) would need the
       patched crates to stay copies.
   - Also consider skipping Windows-only crates (≥136 MB of the 549 MB) if they
     are never compiled.

**Expected result** *(inferred)*: a one-crate change drops from 76.5 s to
roughly evaluation (~7 s) plus one fetch plus a few seconds of merge.

**Then decide separately whether to chase buck2 invalidation (§3.3).**

- First measure it: `tk build //...`, bump one crate, rebuild, and count
  re-run `rust_library` actions.
- If it is confirmed that every vendored crate recompiles, the fix is a
  per-crate *project-side* layout. That means a `.turnkey/rustdeps/` directory
  of per-crate store-path symlinks, each holding source plus `rules.star`, fed
  by a global JSON of features and resolved dependencies (crate2nix JSON style).
- That is a separate execution ticket with FUSE and remote-execution
  implications. It is **not** justified by generation time.

## 7. Not verified

- **How many buck2 actions a one-crate change re-runs** (§3.3). The claim is
  inferred from buck2's `ExternalSymlink` keying, as documented in
  [remote-execution-and-caching.md](remote-execution-and-caching.md), not
  measured.
- **Whether buck2 reads BUCK files and sources through per-crate absolute
  symlinks nested inside a cell.**
- **The exact split of the ~49 s in-Nix bucket** between the BUCK loop and
  output registration. The ~21 s registration figure comes from a separate
  copy-only probe of the same tree. `auto-optimise-store` is suspected but
  not isolated.
- **Linux and sandboxed builds.** All numbers are aarch64-darwin with
  `sandbox = false`. A sandboxed Linux builder adds per-derivation setup
  cost, which would make per-crate derivations more expensive still
  *(inferred)*.
- **Fetch time for a new crate** is not included. The hash-discovery run
  fetched `anyhow` before the timed build.
