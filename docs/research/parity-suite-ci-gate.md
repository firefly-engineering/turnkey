# Is the test-runner parity suite fit to be a CI gate?

Research note for ticket `turnkey-luk.2` (map `turnkey-luk`, "Bundle buck2
inside turnkey"). It feeds `turnkey-luk.8`, which asks whether the parity suite
should be a CI gate for buck2 pin bumps, or a manual checklist step.

Sources: the repo at commit `31ee15e` (change `ovxuxluk`), its git history, the
repo's GitHub Actions run history (`gh run`), and the GitHub-hosted runner
documentation. Timings were measured on 2026-09-25 in a separate jj workspace.
Claims inferred from reading code, rather than observed, are marked
*(inferred)*.

## Answer

- **Speed:** the suite is fast. With the dev shell already built, it takes
  about 3 minutes cold and 6 to 17 seconds warm.
- **Self-containment:** it is not FUSE-dependent. It needs the dev shell, a
  buck2 daemon (which buck2 starts itself), and nix store paths. It needs the
  network only when the store is cold.
- **Blocker:** it cannot run in the repo's current GitHub Actions setup. The
  suite runs inside the full dev shell, and CI dropped every job that
  materialises that shell because the shell exceeded the runners' disk and time
  limits (commit `b64b124`). The shell also contains `turnkey-composed`, whose
  Nix package cannot build on either GitHub runner OS as written.
- **What a gate would need:** a slimmed CI shell, or a dedicated flake output,
  that carries only what `buck2 test //...` needs, plus a cached nix store.
  Without that work, the suite is a manual checklist step. That is what the
  header comment of `nix/buck2/buck2-source.nix` already prescribes.

## What the suite does

The suite lives at `src/cmd/check-test-runner-parity/__main__.py`.

1. It runs `tk sync` once to bring the generated deps files and cells up to
   date.
2. For each of 3 scenarios (`defaults`, `env-and-timeout`, `test-arg`), it runs
   `buck2 test //...` twice. One run uses buck2's bundled runner
   (`-c test.v2_test_executor=`). The other uses turnkey's runner, which the
   generated `.buckconfig` `[test] v2_test_executor` names. Both runs pass
   `-c turnkey.test_cache=false`.
3. After each run it reads `buck2 log show` and compares exit code, per-target
   test status, and per-target action digest.

So the suite makes 6 `buck2 test //...` invocations plus 6 `buck2 log show`
invocations. It exits 0 only if every scenario matches. Commit `6ecf697`
switched it from `tk test` to `buck2 test`, because tk now appends
caching-runner flags that the bundled runner rejects.

On `//...` today, the suite compares 30 test targets. The `test-arg` scenario
exits with code 32 under both runners, because the injected
`--turnkey-parity-probe` argument makes tests fail. The suite compares
outcomes; it does not require tests to pass.

## Measured timings

Measurements were taken on an Apple M5 Max (18 cores, 128 GB RAM) in the fresh
jj workspace `/tmp/turnkey-research-parity-ci`, running
`direnv exec . python3 src/cmd/check-test-runner-parity/__main__.py`. The buck2
release in the shell was 2026-09-15.

| Run | Condition | Wall time | Result |
| --- | --- | --- | --- |
| First direnv entry | nix store warm (shell already built by the main checkout); `use_turnkey` regenerated `go-deps.toml` and `js-deps.toml` | 42.5 s | ok |
| Suite, "cold" | new workspace: empty `buck-out`, new buck2 daemon, nix store warm | 171.5 s | all 3 scenarios match |
| Suite, warm #1 | right after the cold run | 17.2 s | all match |
| Suite, warm #2 | again | 6.3 s | all match |

- After the cold run, `buck-out` was 2.5 GB.
- A truly cold machine (empty `/nix/store`) was not measured. It adds the dev
  shell and cell closure on top of the numbers above. The runtime closure of
  the shell's `PATH` entries plus the 7 cell symlink targets is 296 store paths
  and 5.1 GB unpacked (`nix path-info -r` then `-s`, deduplicated).
- The cells (`godeps`, `rustdeps`, `pydeps`, `jsdeps`, `soldeps`, the
  toolchains cell, the prelude) are project-specific. They are deliberately left
  out of the Cachix publish set (`flake.nix`, comment above `publicPackages`;
  `.github/workflows/cachix.yaml`). A cold CI machine therefore builds them.
  Their fixed-output fetchers download crates, Go modules and npm packages
  *(inferred)*.

## What it needs

| Need | Detail |
| --- | --- |
| Dev shell | `buck2`, `tk`, `python3`, plus the Rust, Go, Python, TypeScript and Solidity toolchains, because `//...` includes tests in all of those languages. Today these come from the full `toolchain.toml` shell, entered with `direnv exec .` or `nix develop --impure`. |
| buck2 daemon | Started implicitly by `buck2 test`, one per workspace. Nothing to provision. |
| Nix store paths | The cells are symlinks into `/nix/store`, for example `.turnkey/rustdeps -> /nix/store/…-rustdeps-cell`. The generated `.buckconfig` hard-codes store paths for `turnkey.test_runner_protocol`, `turnkey.test_path` and `test.v2_test_executor`. |
| Network | Only to fill a cold nix store, including cell fetchers. Also for `godeps-gen --prefetch` when `go.mod`/`go.sum` changed (`.turnkey/sync.toml`). A warm run made no fetches (`tk sync` reported every deps file "ok"). Whether any of the 30 tests reach the network was not audited. |
| FUSE / turnkey-composed / macFUSE | **Not used** by the suite. Details below. |

### Why FUSE is not involved

- Cells are plain nix-store symlinks created by `use_turnkey`, not a FUSE
  mount. See `.turnkey/*` in the workspace, and `docs/architecture/fuse-composition-layer.md`,
  which lists symlinks as the backend for CI.
- Neither `tk sync` nor the direnv library starts or talks to the composition
  daemon. `compose` appears in `src/cmd/tk` only as the separate `tk compose`
  subcommand.
- The test targets under `//...` never compile FUSE code:
  - `src/rust/composition` defines `composition-test` and `integration-tests`
    against the plain `:composition` library, which has no `fuse`/`fuse-t`
    features. Without those features, `is_fuse_available()` is a constant
    `false` (`src/rust/composition/src/selector.rs:248`).
  - The real mount tests are `#[cfg(feature = "fuse")]` and `#[ignore]`
    (`src/rust/composition/tests/integration_tests.rs:262-309`).
  - `composition-full`, the only target that links `-lfuse3`, is used only by
    the `turnkey-composed` binary, which is not a test dependency.

On that basis the suite was run for this note. No macFUSE prompt could be
triggered, because nothing mounts.

## Can it run on the repo's existing GitHub Actions?

### The current CI setup

- `.github/workflows/ci.yaml` runs `lint` (`nix flake check --no-build`),
  `build-prelude` and `build-tools` on `ubuntu-latest`. The most recent run
  (36125884839, 2026-09-25) took about 2.5 minutes end to end.
- `cachix.yaml` pushes `flake.lib.publicPackages` for `x86_64-linux` only.
- `docs.yaml` enters `nix develop .#docs`, a small separate shell.
- `.github/actions/setup-nix` installs Nix (DeterminateSystems
  nix-installer-action v16) with the `firefly-toolbox` and `firefly-turnkey`
  Cachix substituters, plus magic-nix-cache.
- `ci.yaml.disabled` is the older workflow. It ran `nix develop --impure -c tk
  test //...`. It was disabled in `101490d` (2026-01-17, `turnkey-eucb`).

There is no macOS job anywhere.

### The runners

For public repositories (this repo is public), the standard GitHub-hosted
runners are
([GitHub docs](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)):

- `ubuntu-latest`: 4 vCPU, 16 GB RAM, 14 GB SSD, x64.
- `ubuntu-24.04-arm`: the same, on arm64.
- `macos-latest`: 3-core M1, 7 GB RAM, 14 GB SSD.

### Blockers, in order of weight

1. **The full dev shell has already failed in CI.** Commit `b64b124`
   (2026-03-06) removed the `devshell`, `integration` and `e2e` jobs:

   > they all materialize the full devshell via `nix develop --impure`, which
   > builds jujutsu from source (40+ min, runs out of disk space)

   The parity suite runs in exactly that shell. The shell has grown since,
   with `vcs-toolchain` 2 (jj 0.40), `beadwork`, the mdbook toolchain and
   `turnkey-composed`. Whether toolbox's Cachix now serves jj was not checked.

2. **`turnkey-composed` is in `toolchain.toml`, so it is part of the shell,
   and its Nix package cannot build on either runner OS as written.**
   - Linux: the package `nix/packages/turnkey-composed.nix` declares no
     `pkg-config` or `fuse3` inputs. Its Linux dependency is
     `composition = { features = ["fuse"] }`, which pulls in
     `fuser = { features = ["libfuse"] }` (`Cargo.toml:45`). fuser 0.17's
     `build.rs` probes `fuse3`/`fuse` through pkg-config and panics when
     neither is found. `flake.nix`'s comment on `publicPackages` records the
     same limitation.
   - macOS: the package links macFUSE's `/usr/local/lib/libfuse3`, and macOS
     runners cannot install macFUSE (same comment).

   A CI shell has to leave it out. The suite does not need it.

3. **Disk.** The runner guarantees 14 GB of SSD. The workload needs roughly:
   - 5.1 GB of unpacked shell and cell closure;
   - the cells' build-time inputs (vendored crates, Go modules and npm
     packages, built in CI because the cells are not in Cachix);
   - 2.5 GB of `buck-out`.

   That is tight, and the same class of limit ended the old jobs *(inferred)*.

4. **Cold time.** Buck2 alone took about 3 minutes with a warm store on an
   18-core machine. A 4-vCPU runner with a cold store adds cell and toolchain
   realisation on top. With a working binary cache that is tens of minutes;
   without one, and with a toolchain built from source, it is much longer, as
   `b64b124` shows. A bump PR would pay the cold cost every time, unless the
   cells are cached. The GitHub job limit (6 h) is not the constraint;
   usefulness is.

### Once the blockers are removed, it fits CI well

- It needs no FUSE, no privileges, no secrets, no interactive prompts, and no
  daemon beyond buck2's own.
- It is deterministic: exit 0 or 1, with per-scenario output.
- It is scoped to the pin: `nix/buck2/buck2-source.nix` maps each release to
  source revisions, and the suite validates the pinned binary against
  turnkey's runner.

A plausible gate needs three things *(inferred; not built or measured here)*:

1. A job triggered only when `nix/buck2/buck2-source.nix`, the buck2 entry of
   `toolchain.toml`, or the test runner changes.
2. A dedicated CI shell or flake app that includes only `buck2`, `tk`,
   `python3`, the language toolchains and the cells. That means no jj,
   beadwork, mdbook or `turnkey-composed`.
3. The cells pushed to Cachix, or a Nix store cache, so later runs are warm.

`ubuntu-latest` is the natural runner. Linux-only coverage misses any
darwin-specific runner difference, and a macOS runner would need the same
shell slimming plus `turnkey-composed` excluded.

## Facts for the "CI gate or manual checklist step" decision

| Question | Fact |
| --- | --- |
| Warm runtime | 6 to 17 s |
| Cold runtime (store warm) | about 3 min on an M5 Max; slower on a 4-vCPU runner |
| Cold runtime (store cold) | not measured; adds realising about 5 GB of closure plus building the uncached cells |
| FUSE / macFUSE | not needed; safe to run anywhere |
| Network | only to fill the nix store or re-prefetch Go deps |
| Runs in today's CI? | **No.** The full dev shell was removed from CI for disk and time (`b64b124`), and `turnkey-composed` in the shell does not build on the runners. |
| Work to make it a gate | a slim CI shell without `turnkey-composed` and the heavy dev-only tools, cells in Cachix, and a path-filtered workflow job |
| Current documented process | a manual step: run the suite with the new release in the dev shell, and require every scenario to match (`nix/buck2/buck2-source.nix` header) |
