# Bumping the Pinned buck2 Release

Each turnkey revision ships exactly one buck2 release
([ADR 0002](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0002-turnkey-owns-the-buck2-version.md)):
the binary and the upstream prelude built with it, both from toolbox, and
the buck2 source revision they come from. They are named together in one
record, in `nix/buck2/buck2-source.nix`. A bump moves all of them in **one
change**, and nothing lands until the parity suite passes against the new
release.

This checklist records what the move to buck2 2026-09-15 took.

## 1. Update toolbox

Toolbox has to carry the new release first, under its date, for both
`buck2` and `buck2-prelude`:

```bash
nix flake update toolbox
nix eval --quiet --json --impure --expr '
  let r = (builtins.getFlake (toString ./.)).lib.defaultTellerRegistry "aarch64-darwin";
  in { buck2 = builtins.attrNames r.buck2.versions;
       prelude = builtins.attrNames r.buck2-prelude.versions; }'
```

If the date is missing from either list, add it to toolbox first. Don't work
around it in turnkey.

A toolbox update moves every other toolchain as well. Expect fallout that
has nothing to do with buck2. Last time:

- a meta-package changed shape: typescript's default became 7, which has no
  `tsc.js`, so the typescript mapping had to take node and tsc from the
  declared meta-package;
- a pinned version disappeared: nix 2.34.1 was dropped from toolbox;
- devenv moved with toolbox and brought an import-from-derivation, which
  broke `nix flake check --no-build` on a clean store. The fix was to take
  `task.package` from the devenv flake's `devenv-tasks`.

## 2. Fill in the pinned record

In `nix/buck2/buck2-source.nix`, set every field of `pinned` for the new
release:

| Field | Where it comes from |
|---|---|
| `version` | The release date: the key toolbox uses for `buck2` and `buck2-prelude`. |
| `rev` | The buck2 commit the release was built from. `buck2 --version` prints `<date>-<rev>`. |
| `preludeRev` | The release's `prelude_hash` asset: `https://github.com/facebook/buck2/releases/download/<version>/prelude_hash`. |
| `protosHash` | Set it to `lib.fakeHash`, run `nix build .#turnkey-test-runner --no-link`, and copy the hash from the error. |

If toolbox's prelude for that date is a different commit than `preludeRev`,
evaluation fails and names both commits. Fix the toolbox entry.

## 3. Port the prelude patches

Follow `nix/patches/prelude/README.md`: dry-run each patch against the new
upstream prelude, and redo in place the ones that no longer apply. There is
one patch set, for the pinned release only; `nix build .#turnkey-prelude`
fails if any patch doesn't apply, so a forgotten port can't ship unpatched.

Read upstream prelude changes that touch the patched code, not just the
hunks that fail to apply. In 2026-07-01 the new preludes gave tests without
a remote-execution profile a non-caching local executor, which the caching
helper (`nix/buck2/prelude-extensions/test_caching/test_caching.bzl`) now
replaces. A change like that applies cleanly and only shows up in what the
rules hand buck2, so check it:

```bash
python3 src/cmd/check-test-caching/__main__.py
```

It analyses a test target of every cache-safe rule with test result caching
on and off, and must report that every target matches.

## 4. Check the protocol and the parity suite

The test runner's protocol code is regenerated from the new `rev`. Protocol
changes show up as build failures in `turnkey-test-runner`. Changes to the
event log the parity suite reads show up as problems in the suite's output.
In 2026-09-15 buck2 renamed the `TestRun` span, so the suite found no action
digests and reported that the requests weren't compared.

In a fresh shell (`nix develop --impure`):

```bash
buck2 --version     # <version>-<rev>, matching the pinned record
python3 src/cmd/check-test-runner-parity/__main__.py
```

Every scenario must report `matches`. The last line is the summary, for
example:

```text
parity: 3/3 scenarios match on 30 targets (buck2 2026-09-14-6507dd157a6f81a810c48583edf1758dd0c337c5, arm64-darwin)
```

## 5. Check test result caching end to end

```bash
tk build //...
tk test //...        # every test runs; passes are recorded
tk test //...        # every test is reported as recorded (reused without running)
```

Check that recorded results also work from a second checkout of the same
commit.

## 6. Run the CI gates locally

Run what `.github/workflows/ci.yaml`, `docs.yaml` and `cachix.yaml` run:

```bash
nix flake check --no-build --impure
nix build --no-link .#turnkey-prelude .#tk .#godeps-gen .#rustdeps-gen .#pydeps-gen .#jsdeps-gen
nix develop .#docs --impure -c bash -c 'for b in landing user-manual developer-manual; do mdbook build docs/$b --dest-dir $(mktemp -d); done'
nix build --no-link --impure --expr '
  let f = builtins.getFlake (toString ./.); p = f.packages.aarch64-darwin;
  in map (n: p.${n}) f.lib.publicPackages'
```

## 7. Commit

Put the whole bump in one commit: the toolbox update, the pinned record, the
patch set and any fallout. Paste the parity summary line into the commit
message, so the log shows the gate ran, against which release, and on which
platform.
