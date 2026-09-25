# buck2 and buck2-prelude versions in toolbox

Research for ticket `turnkey-luk.1`, part of map `turnkey-luk` (*Bundle buck2 inside turnkey*): *Which buck2 and buck2-prelude versions does toolbox expose?*

- **Researched:** 2026-09-25.
- **Pins:** turnkey commit `31ee15e7` (change `ovxuxluk`). That commit locks toolbox at
  [`064d060baa529d8ea9d02064cc21e28bc862fcdf`](https://github.com/firefly-engineering/toolbox/tree/064d060baa529d8ea9d02064cc21e28bc862fcdf)
  and teller at
  [`417b9fc87e9c4a00f08abb5cd82b86e2dd164727`](https://github.com/firefly-engineering/teller/tree/417b9fc87e9c4a00f08abb5cd82b86e2dd164727).
- **Sources:** the toolbox and teller source at those revisions, an evaluation of turnkey's own
  `lib.defaultTellerRegistry`, and the `prelude_hash` asset attached to each
  [facebook/buck2 release](https://github.com/facebook/buck2/releases).
- **Method:** nothing was built. Only narrow `nix eval` calls were run, with `--apply` to reduce the output to attribute names and `passthru` fields.

## Answer

| Question | Answer |
|---|---|
| Is there a plain `buck2` entry versioned by release date? | **Yes.** Its versions are `2025-12-01`, `2026-03-15`, `2026-04-15`, `2026-07-01` and `2026-09-15`. The default is **`2026-09-15`**. As turnkey resolves the entry, it also has a sixth version, `default`, which is nixpkgs' `buck2-unstable-2025-12-01` from teller. |
| Is there a `buck2-prelude` entry versioned the same way? | **Yes.** Its versions are `2025-12-01`, `2026-03-15`, `2026-07-01` and `2026-09-15`. The default is **`2026-09-15`**. There is **no `2026-04-15`**. |
| Does a prelude exist for every buck2 release? | **No.** `2026-04-15` has no prelude entry. Two of the existing entries also point at the wrong commit. |
| Does the `2026-09-15` prelude exactly match buck2 `2026-09-15`? | **Yes.** Its rev `4d101dce…` is the `prelude_hash` published with that buck2 release. |
| What does `buck2-toolchain` 5 bundle? | buck2 `2026-09-15` and reindeer `2026.09.21.00`. It does not bundle a prelude. The default toolchain is `5`. |

### Checking each prelude against its buck2 release

Every buck2 GitHub release has a `prelude_hash` asset. It holds the
[facebook/buck2-prelude](https://github.com/facebook/buck2-prelude) commit that was built together with that binary. The table compares it with toolbox's prelude entry for the same date.

| Release | Release commit (buck2) | `prelude_hash` (upstream) | toolbox `buck2-prelude` rev | Exact? |
|---|---|---|---|---|
| `2026-09-15` | `6507dd15…` | `4d101dce3482c35b32f9f1e7072b354ae789d256` | `4d101dce3482c35b32f9f1e7072b354ae789d256` | **yes** |
| `2026-07-01` | `c88d791e…` | `2480d82ad5efac0f8a57c708925607ca0791b021` | `2480d82ad5efac0f8a57c708925607ca0791b021` | **yes** |
| `2026-04-15` | `7600cb80…` | `f0896771c4cc1ab8f87e032c5293376c89e5096b` | *(no entry)* | **missing** |
| `2026-03-15` | `00b0e8f2…` | `27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795` | `27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795` | **yes** |
| `2025-12-01` | `75e4243c…` | `0a994e0b600f7d035e1ac69f374c0e37e1e19af6` | `0fabd579c12c585c612ecab4f397b50aae334099` | **no** |

For `2025-12-01`, toolbox's rev `0fabd579` was committed on 2025-11-28 ("Fix oss-enable in apple_test.bzl"). The release's own prelude, `0a994e0b`, was committed on 2025-12-01. The toolbox entry therefore holds an older prelude under the release's name.

For buck2 `2026-04-15`, turnkey's `matchingPreludeVersion` currently falls back to the newest older prelude, which is `2026-03-15` (`27c8628d`). The release's own prelude would be `f0896771`, committed on 2026-04-14.

The `release commit` column matches the revs in turnkey's
[`nix/buck2/buck2-source.nix`](../../nix/buck2/buck2-source.nix) for `2026-09-15`, `2026-07-01` and `2026-04-15`. Those revs come from `gh release view <tag> -R facebook/buck2 --json targetCommitish`.

## Details and sources

### The toolbox entries

toolbox builds its registry by loading every directory under `packages/`
([`flake.nix` L90-L101](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/flake.nix#L90-L101)).
It exposes the registry through `teller.lib.mkRegistryOverlay` (L85-L88). For each entry, `buildPackage` reads the versions from `data.json` (every key except `_meta`) and the default from `_meta.default`
([`lib/default.nix` L4-L12, L116-L125](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/lib/default.nix#L116-L125)).

- **`buck2`** is the upstream prebuilt release binary. It fetches
  `https://github.com/facebook/buck2/releases/download/${version}/buck2-${platform}.zst`, so each version key is itself the release tag
  ([`packages/buck2/default.nix`](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/packages/buck2/default.nix)).
  `_meta.default = "2026-09-15"`, and the versions are listed at L6, L20, L34, L48 and L62 of
  [`packages/buck2/data.json`](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/packages/buck2/data.json).
  Hashes exist for all four supported systems. The derivation names are `buck2-<date>`.
- **`buck2-prelude`** is `fetchFromGitHub { owner = "facebook"; repo = "buck2-prelude"; rev; hash; }`. Its
  `passthru` carries `version` (the date key) and `preludeRev` (the commit)
  ([`packages/buck2-prelude/default.nix` L6-L17](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/packages/buck2-prelude/default.nix#L6-L17)).
  `_meta.default = "2026-09-15"`, and the versions are listed at L6, L10, L14 and L18 of
  [`packages/buck2-prelude/data.json`](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/packages/buck2-prelude/data.json).
  The derivation name is `source`, which is the default for `fetchFromGitHub`, so the name says nothing about the version. Use `passthru.version` and `passthru.preludeRev` instead.
- **`buck2-toolchain`** has versions `1` to `5`, and `_meta.default = "5"`. Version 5 is buck2 `2026-09-15` with reindeer `2026.09.21.00`, and version 4 is buck2 `2026-07-01`
  ([`packages/buck2-toolchain/data.json` L2-L9](https://github.com/firefly-engineering/toolbox/blob/064d060baa529d8ea9d02064cc21e28bc862fcdf/packages/buck2-toolchain/data.json#L2-L9)).
  No toolchain version bundles a prelude.

### How turnkey resolves the registry (teller merge)

`self.lib.defaultTellerRegistry system` applies `teller.overlays.default` and then `toolbox.overlays.default` to turnkey's nixpkgs, and returns `turnkeyRegistry`
([`flake.nix` L60-L67](../../flake.nix)).
teller's base registry already defines `buck2 = single pkgs.buck2`, which is `{ versions."default" = pkgs.buck2; default = "default"; }`
([teller `registry/default.nix` L18-L30](https://github.com/firefly-engineering/teller/blob/417b9fc87e9c4a00f08abb5cd82b86e2dd164727/registry/default.nix#L18-L30)).
`mkRegistryOverlay` merges an entry that already exists by combining the two `versions` sets and letting the newer overlay's `default` win
([teller `lib/default.nix` L105-L127](https://github.com/firefly-engineering/teller/blob/417b9fc87e9c4a00f08abb5cd82b86e2dd164727/lib/default.nix#L105-L127)). This has two consequences:

- The resolved `buck2` entry has a sixth version, `default`, whose derivation is `buck2-unstable-2025-12-01` from nixpkgs. Its default is still `2026-09-15` because toolbox's default wins. A pin must name a date explicitly, never `"default"`.
- The merged entry keeps only `versions` and `default`, and drops toolbox's `toolbox` metadata stamp. teller has no `buck2-prelude` or `buck2-toolchain` entry, so those two pass through from toolbox unchanged.

The evaluation that confirmed this used `aarch64-darwin` and the committed tree:

```
nix eval --quiet --json "git+file://$PWD?rev=31ee15e7…#lib.defaultTellerRegistry" --apply 'f: let r = f "aarch64-darwin"; in
  builtins.mapAttrs (n: e: { versions = builtins.attrNames e.versions; default = e.default; }) { inherit (r) buck2 buck2-prelude buck2-toolchain; }'
```

```
buck2:           ["2025-12-01","2026-03-15","2026-04-15","2026-07-01","2026-09-15","default"]  default "2026-09-15"
buck2-prelude:   ["2025-12-01","2026-03-15","2026-07-01","2026-09-15"]                         default "2026-09-15"
buck2-toolchain: ["1","2","3","4","5"]                                                        default "5"
```

A second evaluation read `passthru.preludeRev` for each prelude version. It returned the revs shown in the table above.

### How turnkey pairs the prelude today

[`nix/flake-parts/turnkey/default.nix` L750-L768](../../nix/flake-parts/turnkey/default.nix) looks up the buck2 version declared in the consumer's `toolchain.toml` (through `buck2-toolchain` or `buck2`). It then asks `buck2Source.matchingPreludeVersion` for a prelude. That function picks the newest prelude date that is not later than the buck2 date, and falls back to the registry default
([`nix/buck2/buck2-source.nix` L53-L69](../../nix/buck2/buck2-source.nix)).
The dates are only a heuristic. They do not prove that a prelude matches its binary, which is how the `2026-04-15` gap and the wrong `2025-12-01` entry go unnoticed.

## Naming the pinned prelude exactly

For the single release turnkey plans to pin, **`2026-09-15`**, nothing needs to change. The pin can use `buck2."2026-09-15"` and `buck2-prelude."2026-09-15"` from turnkey's own resolved registry. That prelude's `passthru.preludeRev` (`4d101dce…`) equals the release's published `prelude_hash`, so the date names it exactly.

Relying on the date key alone is not faithful in general, as the `2025-12-01` entry shows. To make the pin exact and checkable, turnkey can record the expected prelude commit next to the source rev in its per-release table. `buck2-source.nix` already has an entry of the form `"2026-09-15" = { rev = "6507dd15…"; }`. The new field would be `preludeRev = "4d101dce3482c35b32f9f1e7072b354ae789d256";`, taken from the release's `prelude_hash` asset. Turnkey would then resolve `buck2-prelude` at the same date key and assert that `passthru.preludeRev` equals `preludeRev`. With that check:

- The pinned prelude is named by the upstream value that defines "the prelude that matches this release". The date lookup becomes a convenience, not the source of truth.
- A toolbox bump that points the date at another commit fails at evaluation time instead of silently pairing the wrong prelude.
- `matchingPreludeVersion` and its fallback can be removed, because a single pinned release never needs a "closest older" prelude.

If turnkey ever pins a release that toolbox lacks, or has wrong (today `2026-04-15` and `2025-12-01`), the fix belongs in toolbox. It needs a `buck2-prelude/data.json` entry keyed by the release date, with `rev` set to that release's `prelude_hash`. Turnkey should not vendor or override the prelude locally.
