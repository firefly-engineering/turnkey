# Prior art: how other tools key, compose and select per-dependency build fixups

Research for [Prior art: how do other tools key, compose and select
per-dependency build fixups?](https://github.com/firefly-engineering/turnkey/issues/122),
part of map [Dependency fixups as a composable Nix module
interface](https://github.com/firefly-engineering/turnkey/issues/121).
Gathered on 2026-09-27. Every claim cites upstream source or first-party docs
pinned to these commits:

| Project | Commit | Date |
|---|---|---|
| [facebookincubator/reindeer](https://github.com/facebookincubator/reindeer) | [`90a692c0`](https://github.com/facebookincubator/reindeer/tree/90a692c0c1272ae08f9e1b1e73015feee51c6b18) | 2026-09-22 |
| [facebook/buck2](https://github.com/facebook/buck2) (shim fixups) | [`2c0bceea`](https://github.com/facebook/buck2/tree/2c0bceeabeaa07a0c88ef75be3c02e1078277248) | 2026-09 |
| [nix-community/crate2nix](https://github.com/nix-community/crate2nix) | [`5e1ecfd2`](https://github.com/nix-community/crate2nix/tree/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767) | 2026-06-29 |
| [NixOS/nixpkgs](https://github.com/NixOS/nixpkgs) | [`f2d32213`](https://github.com/NixOS/nixpkgs/tree/f2d32213fae9212aca159b8fda8108e3c9fbf366) | 2026-09-27 |
| [cargo2nix/cargo2nix](https://github.com/cargo2nix/cargo2nix) | [`a709c746`](https://github.com/cargo2nix/cargo2nix/tree/a709c74619e1a2b68ed12bb398e12fbe29d69657) | 2025-06-19 |
| [nix-community/poetry2nix](https://github.com/nix-community/poetry2nix) | [`ce2369db`](https://github.com/nix-community/poetry2nix/tree/ce2369db77f45688172384bbeb962bc6c2ea6f94) | 2025-04-03 |

*(inferred)* marks a conclusion drawn from the sources rather than stated by
them.

## TL;DR

- **Two models exist.**
  - **Data** (reindeer): a fixup is a declarative TOML record with a fixed
    schema. The tool interprets it. It can be checked for unused entries.
  - **Function** (nixpkgs `buildRustCrate`, crate2nix, cargo2nix, poetry2nix):
    a fixup is a Nix function that rewrites the arguments or attributes of a
    per-crate derivation. It can do anything, and it can't be inspected.
  - turnkey sits between the two: shell-string functions for build scripts,
    plain data for rustc flags and native libraries.
- **Everyone keys by crate name.** Version selection is a *predicate inside*
  the fixup, not a key:
  - reindeer: `['cfg(version = ">=0.18")']` sections;
  - nixpkgs: `lib.versionAtLeast attrs.version "2.0"` in the function body;
  - cargo2nix: the only one with a key-level `version` matcher, and it is exact
    equality.
  - Nobody uses turnkey's `name@version` key.
- **Composition:**
  - **nixpkgs and crate2nix** compose with a shallow `defaults // mine`, like
    turnkey does today. A consumer entry *replaces* the default for that crate
    wholesale, unless the consumer calls the default by hand.
  - **cargo2nix and poetry2nix** compose an **ordered list** of overrides. Each
    one sees the previous one's output, so overrides of the same crate *stack*.
  - **reindeer** does not compose at all: one `fixups_dir`, one file per crate.
- **Opting out:**
  - nixpkgs and crate2nix: set the crate's entry to `_: { }`, or don't start
    from `defaultCrateOverrides`.
  - cargo2nix: pass a list without the entry. Overrides are opaque functions,
    but `rustBuilder.overrides` also exposes them by name.
  - poetry2nix: `withoutDefaults` drops the whole default set. `overrideOverlay`
    replaces some of its entries.
- **Platform:**
  - reindeer is **multi-platform in one output.** `cfg(...)` sections are
    evaluated per configured platform, and the results go into platform-keyed
    attributes of the generated rules.
  - The Nix tools are **single-platform per evaluation.** Overrides branch on
    `stdenv.hostPlatform` (cargo2nix, nixpkgs), never on
    `builtins.currentSystem`.
- **Third-party sets:**
  - Only the Nix function model makes a set importable in practice. nixpkgs'
    `defaultCrateOverrides`, cargo2nix's `overrides.all` and poetry2nix's
    `defaultPoetryOverrides` are all "a set someone else publishes and you
    extend".
  - reindeer sets are shared by **copying directories**, e.g. buck2's own
    `shim/third-party/rust/fixups`, with 127 crate entries.
- **Patches:**
  - Nix tools accept `patches`/`postPatch` in the override, because it is just a
    derivation attribute.
  - reindeer has no patch field. You replace files through an `overlay`
    directory, or redirect the crate through Cargo `[patch]`.

## 1. buck2 reindeer

Sources: the reindeer manual, [`docs/MANUAL.md` §Fixups–§Version-specific
fixups](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/docs/MANUAL.md#fixups),
and the loader,
[`src/fixups/config.rs`](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups/config.rs)
and
[`src/fixups.rs`](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs).

**Keying.**
- A fixup lives at `fixups/<package name>/fixups.toml`. "The package name is the
  name only, not including version" (MANUAL §Fixups).
- `FixupsCache::get` joins `resolved_fixups_dir()` with `package.name`
  ([`src/fixups.rs` L128–153](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs#L128-L153)).
- Version selection happens *inside* the file, as a section keyed by a
  predicate: `['cfg(version = ">=0.18")']` (MANUAL §Version-specific fixups).
  It is a semver `VersionReq`
  ([`src/platform.rs` L232](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/platform.rs#L232)).
- Platform and version predicates share one syntax. Any section whose key starts
  with `cfg(` is a conditional fixup
  ([`src/fixups/config.rs` L581–582](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups/config.rs#L581-L582)).

**Contents.** The schema is fixed (`FixupConfig`, with
`#[serde(deny_unknown_fields)]` on the build-script structs):
- **Sources:** `extra_srcs`, `omit_srcs`, `overlay`.
- **Rustc:** `rustc_flags`, `rustc_flags_select`, a map from a Buck constraint to
  flags.
- **Cfgs:** `cfgs`. They are also fed into conditional-dependency resolution,
  which is why they are distinct from `rustc_flags`.
- **Features:** `features`, `omit_features`.
- **Deps:** `extra_deps`, `omit_deps`.
- **Env:** `env`, `cargo_env`.
- **Linking:** `link_style`, `preferred_linkage`, `linker_flags`.
- **Build scripts:** `buildscript.run = true|false`, `[buildscript.build]`
  (extra deps and env for compiling build.rs), and `[buildscript.run.env]`.
  `buildscript.run` can also translate `cargo::rustc-link-lib` and
  `rustc-link-search` into flags
  ([`src/fixups/buildscript.rs` L30–75](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups/buildscript.rs#L30-L75)).
- **Native code:** `[[cxx_library]]` builds C/C++/asm from the crate's sources.
  `[[prebuilt_cxx_library]]` links shipped static libraries (MANUAL §C++
  dependencies).

Reindeer *runs* build scripts as Buck actions
(`<crate>-build-script-run`, which exposes `[rustc_flags]` and `[out_dir]`
sub-targets). It does not accept pre-generated `OUT_DIR` content as a fixup
field. `overlay` is the nearest equivalent: it is a fixup-relative directory
whose files are added to, or replace, the crate's sources.

**A build script needs an explicit decision.** If a crate has a build.rs and no
fixup says `buildscript.run = true` or `false`, buckify reports "has a build
script, but fixups/<pkg>/fixups.toml does not say what to do with it". With
`unresolved_fixup_error` set, that is a hard error
([`src/fixups.rs` L479–500](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs#L479-L500)).
Unused entries are also reported, per file and line, by `UnusedFixups`: a
buildscript fixup on a crate without a build script, globs that match nothing,
and unused visibility
([`src/unused.rs`](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/unused.rs)).

**Composition.**
- There is **exactly one source.** `fixups_dir` is an `Option<PathBuf>`
  ([`src/config.rs` L65](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/config.rs#L65)).
  It is resolved relative to `reindeer.toml`, or defaults to
  `<third_party_dir>/fixups`
  ([L568–573](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/config.rs#L568-L573)).
  There is no list of dirs, no layering and no "defaults".
- The only merging is *within one file*. `configs()` returns the base section
  plus every `cfg(...)` section whose predicate holds, in file order
  ([`src/fixups.rs` L170–187](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs#L170-L187)).
  - List and set fields are unioned: features, cfgs, rustc_flags and deps
    ([L1135–1190](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs#L1135-L1190)).
  - `env` maps are extended.
  - Scalars such as `link_style` are last-set-wins in file order
    ([L1722–1746](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups.rs#L1722-L1746)).
- `cargo_env` can also be set globally in `reindeer.toml` as a default for
  crates whose fixup does not specify it (MANUAL §reindeer.toml). That is the
  only global-default mechanism.

**Opt-out.** There is no notion of a default to opt out of. You edit or delete
the file.

**Per-platform.**
- Platforms are declared in `reindeer.toml` as `[platform.<name>]`, from a
  target triple or explicit `target_*` cfg values (MANUAL §Custom platforms).
  Buckify evaluates each `cfg(...)` section per platform.
- Results are grouped by platform (`configs_grouped_by_platform`, L206–228).
  They are emitted into the `platform: BTreeMap<PlatformName, …>` attributes of
  the generated rules
  ([`src/buck.rs` L262, L590, L916](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/buck.rs#L590)).
  So **one buckify emits every platform**, selected at Buck configuration time.
- Some fields may not be platform-specific (visibility, omit_targets,
  precise_srcs, compatible_with, …), and the loader rejects them in a `cfg`
  section
  ([`src/fixups/config.rs` L86–152](https://github.com/facebookincubator/reindeer/blob/90a692c0c1272ae08f9e1b1e73015feee51c6b18/src/fixups/config.rs#L86-L152)).

**Third-party sets.** Nothing in reindeer imports a set from elsewhere.
Communities share by copying: buck2's own Rust third-party shim carries 127
crate fixup directories
([`shim/third-party/rust/fixups`](https://github.com/facebook/buck2/tree/2c0bceeabeaa07a0c88ef75be3c02e1078277248/shim/third-party/rust/fixups)).
*(inferred)* Pointing `fixups_dir` at a shared checkout lets you reuse a set,
but not extend it.

**Patches.** There is no patch field. The manual sends source changes through
Cargo `[patch.crates-io]` to a git fork (MANUAL §Cargo.toml syntax), or through
`overlay` file replacement. The manual links to a `#Local-Patches` section that
does not exist at this commit.

## 2. nixpkgs `buildRustCrate` / `defaultCrateOverrides`

Sources:
[`build-rust-crate/default.nix`](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/pkgs/build-support/rust/build-rust-crate/default.nix),
[`default-crate-overrides.nix`](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/pkgs/build-support/rust/default-crate-overrides.nix),
and the manual,
[`doc/languages-frameworks/rust.section.md` §Handling external dependencies](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/doc/languages-frameworks/rust.section.md#handling-external-dependencies).

**Keying.**
- The mechanism is one line:
  `crate = crate_ // (lib.attrByPath [ crate_.crateName ] (attr: { }) crateOverrides crate_);`
  ([L360](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/pkgs/build-support/rust/build-rust-crate/default.nix#L360)).
- The key is the crate name only. The manual says "the key is the crate name
  without version number and the value a function".
- Version selection is a predicate in the body. For example, `proc-macro-crate`
  uses `lib.optionalAttrs (lib.versionAtLeast attrs.version "2.0") { … }`
  ([default-crate-overrides.nix L415](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/pkgs/build-support/rust/default-crate-overrides.nix#L415)).

**Contents.**
- An override is anything the function returns, shallow-merged over the crate's
  arguments. In practice:
  - `buildInputs` and `nativeBuildInputs`, for native libraries;
  - `propagatedBuildInputs`, which flow to every dependent crate (L490–498);
  - `extraRustcOpts`;
  - `patches`, `prePatch` and `postPatch` (see `evdev-sys` and
    `proc-macro-crate`);
  - env vars.
- Anything that is not a processed attribute passes through to `mkDerivation`
  (`extraDerivationAttrs`, L361–388).
- `buildRustCrate` runs build.rs itself, so overrides rarely fake its output.
  They supply what build.rs needs.

**Composition.**
- `crateOverrides` is one attrset, and the documented idiom is
  `defaultCrateOverrides // { hello = attrs: { … }; }`.
- A consumer entry therefore **replaces the default function for that crate
  wholesale**. *(inferred)* To keep a default's effect, the consumer has to call
  it by hand: `foo = a: (defaultCrateOverrides.foo a) // { … }`.
- The function's result is also `//`-merged over the crate. Setting
  `buildInputs` replaces the list, it doesn't append, which is why the defaults
  concatenate with `attrs.postPatch or ""` themselves.
- The set is forwarded to the whole dependency tree: "the `crateOverrides`
  parameter is forwarded to the crate's dependencies" (manual).

**Opt-out.**
- Override one entry with `_: { }`, or don't start from
  `defaultCrateOverrides`.
- The default is baked in with
  `crateOverrides = defaultCrateOverrides` (L668), and replaced with
  `buildRustCrate.override { defaultCrateOverrides = …; }`.

**Per-platform.** Single-platform per evaluation. Overrides branch on
`stdenv.buildPlatform` and `stdenv.hostPlatform`: `evdev-sys` adds autotools
only when cross compiling (L114). The build-script environment is derived from
`stdenv.hostPlatform.rust.platform` (`configure-crate.nix` L147–158). Nothing
reads `builtins.currentSystem`.

**Third-party sets.** `pkgs.defaultCrateOverrides` is itself the canonical
published set. crate2nix, below, imports and extends it.

## 3. crate2nix

Sources:
[`docs/…/30_crateOverrides.md`](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/docs/src/content/docs/30_building/30_crateOverrides.md),
the generated-code template
[`crate2nix/templates/nix/crate2nix/default.nix` L227–300](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/templates/nix/crate2nix/default.nix#L227-L300),
[`Cargo.nix.tera` L14–15](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/crate2nix/templates/Cargo.nix.tera#L14-L15),
and
[`lib/build-from-json.nix` L25, L179](https://github.com/nix-community/crate2nix/blob/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767/lib/build-from-json.nix#L179).

crate2nix adds **no fixup mechanism of its own**. The generated `Cargo.nix`
takes `defaultCrateOverrides ? pkgs.defaultCrateOverrides` and passes it to
`buildRustCrate.override { defaultCrateOverrides = …; }`. The newer JSON path
does the same. The docs state it directly: "`crateOverrides` are a feature of
the underlying `buildRustCrate` support in NixOS". So keying, contents,
composition (`pkgs.defaultCrateOverrides // { … }`), opt-out and platform
handling are exactly those of §2.

The documented platform idiom is ordinary Nix,
`lib.optionals pkgs.stdenv.isDarwin [ … ]`, inside the function. The docs
concede that patches "should also" work but were never tried.

## 4. cargo2nix

Sources:
[`overlay/lib/overrides.nix`](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/overlay/lib/overrides.nix),
[`overlay/overrides.nix`](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/overlay/overrides.nix),
[`overlay/make-package-set/internal.nix` L16, L55–61](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/overlay/make-package-set/internal.nix#L55-L61),
and the
[README §Common issues](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/README.md).

**Keying.**
- `makeOverride { registry ? null, name ? null, version ? null, overrideArgs ? null, overrideAttrs ? null }`.
  The matcher is the subset of `registry`, `name` and `version` you supplied,
  compared by **exact equality** against the crate's arguments
  (`builtins.intersectAttrs matcher args == matcher`).
- So an override can be keyed by name, by name plus exact version, by registry
  only (the global `capLints` override matches every crates.io crate), or by
  nothing.
- There are no version ranges. For anything richer, write the
  `args -> { overrideArgs; overrideAttrs; }` function yourself.

**Contents.**
- Two overriders:
  - `overrideArgs` rewrites the arguments of `mkRustCrate`, e.g.
    `rustcLinkFlags`;
  - `overrideAttrs` rewrites the resulting derivation, e.g.
    `propagatedBuildInputs`, `postConfigure` or env.
- Helper patches (`patchOpenssl`, `propagateEnv`, which exports env vars
  through a setup hook) live beside the overrides. cargo2nix also runs build.rs
  itself.

**Composition.**
- `packageOverrides` is an **ordered list** (default: `rustBuilder.overrides.all`).
  It is folded with
  `builtins.foldl' rustLib.combineOverrides rustLib.nullOverride packageOverrides`.
- `combineOverriders a b = old: let attrs = old // a old; in attrs // b attrs`.
  Later overrides see and extend earlier ones' results. Two overrides of the
  same crate **stack**, and the last one wins on a key conflict.
- Every override is written as `drv.x or [ ] ++ [ … ]` precisely so that it
  composes.
- The consumer idiom is
  `packageOverrides = pkgs: pkgs.rustBuilder.overrides.all ++ [ (makeOverride { … }) ]`.

**Opt-out.** Pass a list without the unwanted override. The list elements are
opaque functions. *(inferred)* But `overrides.nix` is a `rec` attrset exposed
as `rustBuilder.overrides`, so its entries can be picked by name (`.ring`,
`.openssl-sys`) to build a filtered list.

**Per-platform.** Branch on `pkgs.stdenv.hostPlatform` when the override is
*built*. `ring = if pkgs.stdenv.hostPlatform.isDarwin then makeOverride { … } else nullOverride;`
([L288–295](https://github.com/cargo2nix/cargo2nix/blob/a709c74619e1a2b68ed12bb398e12fbe29d69657/overlay/overrides.nix#L288-L295)).
`packageOverrides` is a function of `pkgs`, so it is evaluated per target
package set.

**Third-party sets.** A set is a list of functions. Any flake can export one,
and consumers concatenate lists. `overrides.all` is the built-in instance.

## 5. Brief: overlay- and module-style composition

**poetry2nix**
([`default.nix` L255–300, L460–520](https://github.com/nix-community/poetry2nix/blob/ce2369db77f45688172384bbeb962bc6c2ea6f94/default.nix#L460-L520),
[README §overrides](https://github.com/nix-community/poetry2nix/blob/ce2369db77f45688172384bbeb962bc6c2ea6f94/README.md#overrideswithdefaults)).
- Overrides are nixpkgs-style overlays `final: prev: { pkg = prev.pkg.overridePythonAttrs …; }`,
  keyed by normalised package name. A list of them is folded with
  `lib.foldr lib.composeExtensions`.
- The defaults ship as `defaultPoetryOverrides`, a functor with two methods:
  - `.extend overlay` layers more on top;
  - `.overrideOverlay fn` replaces individual entries (`defaultSet // customSet`).
- Consumers choose `overrides.withDefaults overlay` (`[ overlay defaults ]`) or
  `overrides.withoutDefaults overlay`. That is the clearest precedent for an
  all-or-nothing opt-out *plus* per-entry replacement.

**NixOS module system**
([`lib/modules.nix` L1644–1772](https://github.com/NixOS/nixpkgs/blob/f2d32213fae9212aca159b8fda8108e3c9fbf366/lib/modules.nix#L1644-L1772)).
- If fixups become an `attrsOf (submodule …)` option, composition across
  imports is defined for free:
  - **Priorities:** `mkOptionDefault` = 1500, `mkDefault` = 1000, a plain
    definition = 100, `mkForce` = 50. The lowest number wins.
  - **List order:** `mkBefore` = 500, the default = 1000, `mkAfter` = 1500.
  - Conflicting scalars at equal priority are an error, not a silent pick.
- *(inferred)* Built-ins would be defined at `mkDefault`, so a consumer's plain
  definition overrides a field without replacing the whole record. A third
  party's set is just another module imported from its flake.
- **Opt-out** takes an explicit `enable` flag per entry. A module definition
  cannot be deleted.

## 6. Comparison

| | Key | Version selection | Contents | Composition | Opt-out | Platform | Importable set |
|---|---|---|---|---|---|---|---|
| reindeer | dir name = crate name | `cfg(version = "<req>")` section | fixed TOML schema; runs build.rs | none (single dir); within-file union / last-wins | edit the file | `cfg(...)` sections, all platforms in one output | copy the dir |
| nixpkgs / crate2nix | attr name = crate name | predicate in function body | any derivation attr, patches | `defaults // mine`, replace per crate | `_: { }` or drop defaults | `stdenv.hostPlatform`, one per eval | yes (`defaultCrateOverrides`) |
| cargo2nix | `{registry,name,version}` exact match | exact version, or custom fn | overrideArgs + overrideAttrs | ordered list, stacking | filter the list | `hostPlatform`, one per eval | yes (list of fns) |
| poetry2nix | overlay attr name | predicate in body | any | overlay chain | `withoutDefaults` / `overrideOverlay` | `stdenv` | yes (overlays) |
| **turnkey today** | `name` or `name@version` | exact `@version` key | 3 separate attrsets: build-script shell string, rustc flags, native-lib info | `builtin // consumer`, per piece | none (built-ins always on) | rustc flags: `{ linux; macos; }` dict → `select()`; build scripts: `builtins.currentSystem` | no |

## Implications for turnkey

These are measured against
[`nix/lib/deps-cell/fixups/rust/default.nix`](../../nix/lib/deps-cell/fixups/rust/default.nix),
[`nix/lib/deps-cell/adapters/rust.nix`](../../nix/lib/deps-cell/adapters/rust.nix)
(`mkRustDepsCell`),
[`nix/buck2/languages.nix`](../../nix/buck2/languages.nix) (the `rust` entry's
`mkCell`) and
[`nix/buck2/options.nix`](../../nix/buck2/options.nix)
(`rustcFlagsRegistry`, `buildScriptFixups`).

1. **Make one record per crate.**
   - Every surveyed tool keeps everything about a crate in one place: one
     `fixups.toml`, or one override function.
   - turnkey splits a crate across `buildScriptFixups`, `rustcFlags` and
     `nativeLibraries`, joined only by key. `ring`'s build script and native
     library live in two attrsets of one file. `nativeLibraries` has no consumer
     option at all (`adapters/rust.nix` L98).
   - reindeer's schema is a good checklist for the record's fields: build
     script (run / don't run / pre-generated `OUT_DIR`), `cfgs` vs raw
     `rustc_flags`, env, features, extra deps, native libs as `cxx_library` /
     `prebuilt_cxx_library`, and an overlay/patches field.
2. **Key by name and select versions with a predicate.**
   - The `name@version` exact key has no precedent except cargo2nix's
     exact-match field. It breaks on every patch bump.
   - reindeer's `cfg(version = ">=x")` and nixpkgs' `versionAtLeast` both let
     one entry cover a range and fork only where behaviour changed.
   - A Nix-native form would be a list of `{ versions = ">=0.17"; … }` sections
     inside the record, or a function of `{ version, … }`.
3. **Merge fields, don't replace the whole entry.**
   - turnkey's `builtin // consumer` is the nixpkgs model, with its known flaw:
     a consumer who wants to add one cfg flag to `rustix` silently discards the
     built-in flags.
   - cargo2nix's ordered stacking and the module system's per-field priorities
     both avoid this.
   - The module system also *errors* on equal-priority scalar conflicts rather
     than picking one, which matches map #121's "defined precedence and conflict
     handling".
4. **Opt out explicitly, at two granularities.**
   - poetry2nix's `withDefaults` / `withoutDefaults` covers the whole set, and
     `overrideOverlay` covers one entry.
   - turnkey needs both: "no turnkey built-ins", and "not turnkey's `ring`".
   - In a module interface, that is a per-set enable plus a per-entry `enable`,
     because module definitions cannot be deleted.
5. **Pass the platform in, or emit per-platform output.**
   - There are two sound precedents:
     - reindeer evaluates predicates per configured platform and emits
       everything for Buck to `select()`;
     - the Nix tools take `stdenv.hostPlatform`.
   - Nobody reads `builtins.currentSystem`, which `ring.nix` does. It also
     breaks pure evaluation.
   - turnkey's Buck output is multi-platform: `conditions` feeds `select()`s,
     and rustix's `{ linux; macos; }` already becomes a select. So the reindeer
     model fits the *data* fields.
   - Build-script fixups that *compile* code, like ring's, happen at Nix build
     time for one platform. *(inferred)* Those need the target platform as an
     explicit input, and one derivation per platform in `conditions`.
6. **Keep the data model inspectable.**
   - reindeer's unused-fixup and missing-buildscript-decision errors are only
     possible because fixups are data.
   - turnkey's build-script fixups are opaque shell strings. Keeping every
     *other* field declarative (flags, env, native libs, patches) preserves the
     ability to validate. For example, it can warn when a fixup names a crate
     absent from `rust-deps.toml`.
7. **Treat a third-party set as a value.**
   - nixpkgs, cargo2nix and poetry2nix all publish a set as a plain Nix value
     (attrset, list or overlay) that a flake can export and a consumer can
     import. That is the "import a set published by a third flake" requirement
     in #121.
   - reindeer shows the cost of not having this: sets are copied, and they
     drift.
8. **Fold patches into the record.**
   - In Nix tools, patches are just another override field.
   - turnkey's separate `.turnkey/patches/<cell>/` mechanism
     (`userPatchesDir`) could stay for the FUSE edit layer. A fixup-carried
     `patches` list fits the record naturally, and #121 puts patches in scope.
