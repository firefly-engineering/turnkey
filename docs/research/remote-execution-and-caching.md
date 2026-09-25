# Remote caching and remote execution for turnkey's buck2 builds and tests

Research note, not tied to a ticket. The question: how could turnkey use a
remote cache or remote-execution (RE) service so that builds and tests skip work
already done on another machine: dev laptops (macOS, Linux) and, above all,
CI? What is missing in turnkey today?

- **Researched:** 2026-09-25.
- **Pinned buck2:** release `2026-09-15`, commit
  [`6507dd15`](https://github.com/facebook/buck2/tree/6507dd157a6f81a810c48583edf1758dd0c337c5)
  ([`nix/buck2/buck2-source.nix` L18-L26](../../nix/buck2/buck2-source.nix#L18-L26)),
  with prelude [`4d101dce`](https://github.com/facebook/buck2-prelude/tree/4d101dce3482c35b32f9f1e7072b354ae789d256).
  Unless stated otherwise, every buck2 link is a permalink at `6507dd15`. The
  earlier notes read `7600cb80` (2026-04-15) and `b8ae22e6` (main). Each claim
  reused from them was re-checked at `6507dd15` where it matters here.
- **REAPI:** [`bazelbuild/remote-apis@adbf4a27`](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto).
- **Method:** source reading, one `direnv exec . env` to see the dev-shell
  environment, and a look at the generated `.buckconfig` and toolchains cell.
  Nothing was built, and no remote service was used. Statements not read off a
  source are marked *(inferred)*; statements that could not be checked are
  marked *(unverified)*.

This note builds on, and does not repeat:

- [local-re-api-servers.md](local-re-api-servers.md): which REAPI servers
  exist, how each behaves, and what buck2's RE client requires (TCP, SHA256,
  `engine_address`, platform only on `Command`, buck2#862).
- [buck2-pinned-test-caching.md](buck2-pinned-test-caching.md) and
  [test-cache-mechanism-feasibility.md](test-cache-mechanism-feasibility.md):
  how buck2 looks up and uploads action and test results.
- [test-undeclared-inputs.md](test-undeclared-inputs.md): what turnkey's tests
  read outside their key.
- [bazel-test-result-caching.md](bazel-test-result-caching.md): Bazel's model.
- [parity-suite-ci-gate.md](parity-suite-ci-gate.md): why CI cannot run the
  dev shell today.
- [ADR 0001](../adr/0001-runner-recorded-native-test-caching.md) and the
  [test result caching spec](../specs/test-result-caching.md), which list
  "caching build action results", "shared-cache handover" and "re-enabling
  tests in CI" as out of scope.

## 1. TL;DR

**Recommendation: start with a shared *remote cache only* (local execution
everywhere), with only CI writing to it, on Linux first. Don't build remote
execution yet.** Turnkey already has most of the client side. What stops a
shared cache from being *correct* today is that build actions have hidden
inputs, not the choice of server.

1. **A shared cache is safe only if everything that decides an output is in the
   action key.** Today it isn't:
   - `rustc` and `go` are resolved from the daemon's `PATH`
     (see [§3.3](#33-what-turnkeys-actions-put-in-the-key-and-what-they-leave-out)).
   - Build actions inherit the whole dev-shell environment (`NIX_CFLAGS_COMPILE`,
     `NIX_LDFLAGS`, `SDKROOT`, `MACOSX_DEPLOYMENT_TARGET`, …), and none of it is
     in the key.
   - An action whose command names no Nix store path gets the same key on
     macOS and Linux. `go_copy_goroot` is one: a Linux CI entry would be served
     as a Mac laptop's GOROOT.

   A laptop-local cache hides these hazards: one machine, one dev shell at a
   time. A shared cache turns them into wrong hits across branches, machines
   and operating systems.
2. **The Nix side is an asset for caching.** Toolchains and dependency cells
   enter the key as absolute `/nix/store/<hash>-…` strings (toolchain paths in
   argv, cells as REAPI `SymlinkNode`s). The same `flake.lock` on the same
   system gives the same strings on every machine. So keys match between Linux
   CI and Linux laptops, and the store hash stands in for the toolchain's
   content.
3. **The same asset blocks remote execution.** buck2 never uploads what an
   external symlink points to. A worker must already have every referenced
   `/nix/store` path, and `rustc`/`go` must stop coming from `PATH`, because
   RE actions get no inherited environment at all. Workers therefore need
   Nix-provisioned toolchains. That is feasible (see §4b), but it is a
   project; the cache alone gets most of the CI benefit.
4. **CI must run buck2 before any of this helps.** Today it doesn't:
   `ci.yaml` only evaluates the flake and builds a few Nix packages
   ([`.github/workflows/ci.yaml`](../../.github/workflows/ci.yaml)), and the
   full dev shell was dropped from CI for disk and time
   ([parity-suite-ci-gate.md](parity-suite-ci-gate.md#blockers-in-order-of-weight)).
   The slim CI shell in open ticket `turnkey-kxe` is the prerequisite.
5. **Server choice is secondary and reversible.** Keys are computed by buck2
   alone, so the backend can be swapped without invalidating anything
   ([local-re-api-servers.md](local-re-api-servers.md#what-goes-into-the-cache-key)).
   Two good starting points:
   - **Roll-your-own lite:** one `bazel-remote`, which turnkey already ships
     for test caching, backed by S3/GCS or a small VM. CI authenticates to
     write; laptops read anonymously (`--htpasswd_file` plus
     `--allow_unauthenticated_reads`).
   - **Hosted:** BuildBuddy (free tier, read-only keys) or BuildFetch, via `http_headers`. NativeLink Cloud no longer exists.

   See [§4](#4-options) for the comparison.
6. **Tests:** turnkey's runner already records test results by buck2's own
   digest. A shared test cache mainly needs the runner to write to a remote
   endpoint under a trust policy (CI only). The spec deferred exactly that
   ([spec §7](../specs/test-result-caching.md#7-shipping-and-configuration)).

## 2. What turnkey caches today

| Layer | Mechanism | Scope | Source |
|---|---|---|---|
| Nix store: tools, prelude, deps generators | Cachix `firefly-turnkey`, pushed from `main` for `x86_64-linux` only; `flake.lib.publicPackages` | shared, Linux only | [`.github/workflows/cachix.yaml`](../../.github/workflows/cachix.yaml), [`flake.nix` L81-L106](../../flake.nix#L81-L106) |
| Nix store: dependency cells (`*-cell`), toolchains cell, dev shell | Not pushed ("intentionally excluded"). CI's `setup-nix` pulls Cachix and uses `magic-nix-cache-action@v8` | per-machine | [`cachix.yaml`](../../.github/workflows/cachix.yaml), [`.github/actions/setup-nix/action.yml`](../../.github/actions/setup-nix/action.yml) |
| buck2 build actions | In-daemon DICE plus buck-out. The execution platform is upstream `prelude//platforms:default`, `local_enabled = True, remote_enabled = False`, so there are no action-cache lookups or uploads | per checkout, per daemon | [generated `.buckconfig` `[build]`](../../nix/devenv/turnkey/buck2.nix#L406-L407), [prelude `platforms/defs.bzl` L21-L25](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/platforms/defs.bzl#L21-L25) |
| buck2 tests | Test result caching: `turnkey-test-runner` records passes into a per-user `bazel-remote` on `127.0.0.1:47301`; buck2 looks them up natively. Only tests use this executor | per user per machine; a remote endpoint can be *read* | [`buck2.nix` L352-L424](../../nix/devenv/turnkey/buck2.nix#L352-L424), [`test_caching.bzl`](../../nix/buck2/prelude-extensions/test_caching/test_caching.bzl), [spec](../specs/test-result-caching.md) |
| CI buck2 builds and tests | **None.** No job runs buck2 | — | [`ci.yaml`](../../.github/workflows/ci.yaml); old jobs in [`ci.yaml.disabled`](../../.github/workflows/ci.yaml.disabled) |

Details that matter for what follows:

- **The generated `.buckconfig` already has a `[buck2_re_client]` section**,
  but only while test caching is on. It points all three endpoints at the
  test-cache address, with `tls` on only for a remote endpoint
  ([`buck2.nix` L418-L423](../../nix/devenv/turnkey/buck2.nix#L418-L423)).
  There is no option for `http_headers`, `instance_name`, `tls_client_cert` or
  `tls_ca_certs`. Hosted services authenticate with one of these.
- **`testCache.endpoint` replaces the local server; it doesn't tier.** With a
  remote endpoint, tk starts no local cache, and the runner records nothing
  ([`buck2.nix` L554-L571](../../nix/devenv/turnkey/buck2.nix#L554-L571)).
  The runner decides "local" by the host being `127.0.0.1`, `localhost` or
  `[::1]`
  ([`runner.rs` L275-L288](../../src/cmd/turnkey-test-runner/src/runner.rs#L275-L288)).
  Its recorder has no TLS and no header support
  ([`cache.rs` L57-L75](../../src/cmd/turnkey-test-runner/src/cache.rs#L57-L75)).
- **`tk` passes its whole environment to buck2** (`syscall.Exec(…, os.Environ())`,
  [`src/cmd/tk/main.go` L540](../../src/cmd/tk/main.go#L540)). The daemon
  therefore inherits the full dev shell.
- **Cells reach buck2 as symlinks into `/nix/store`** (`.turnkey/godeps ->
  /nix/store/…-godeps-cell`), declared as plain `[cells]`
  ([`buck2.nix` L296-L325](../../nix/devenv/turnkey/buck2.nix#L296-L325)).
  The FUSE backend (epic `turnkey-4vl`) is not used by CI or tests
  ([parity-suite-ci-gate.md](parity-suite-ci-gate.md#why-fuse-is-not-involved)).
  Its design wants a fixed mount point "for remote caching"
  ([`fuse-composition-layer.md` L138-L147](../architecture/fuse-composition-layer.md#L138-L147)),
  but §3 shows buck2 keys don't need it: they are already project-relative
  plus store paths.

## 3. How cache keys work, and what that means for Nix toolchains

### 3.1 REAPI

- **The action cache is keyed by the `Action` digest.** An `Action` holds:
  - the `Command` digest: arguments, environment variables, output paths,
    working directory and (deprecated there) the platform;
  - the input-root digest;
  - `timeout`, `do_not_cache`, `salt` and `platform`

  ([`remote_execution.proto` L674-L739, L749-L886](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L674-L739)).
- **The platform is part of the key.** "The platform is implicitly part of the
  action digest, so even tiny changes in the names or values … may result in
  different action cache entries" ([L934-L960](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L934-L960)).
- **Anything else is "defined by and specific to the implementation"**: which
  system binaries exist, which filesystems are mounted where
  ([L742-L748](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L742-L748)).
  A `/nix/store` path in argv is one such thing. The key names it; the worker
  must provide it.
- **`salt`** exists "to place this Action into a separate cache namespace …
  and allows disowning an entire set of ActionResults that might have been
  poisoned" ([L721-L729](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L721-L729)).
  buck2 never sets it
  ([local-re-api-servers.md](local-re-api-servers.md#what-goes-into-the-cache-key)).
- **`UpdateActionResult` requires the `Action` and `Command` in the CAS first**,
  so that servers can apply access control
  ([L174-L192](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L174-L192)).

### 3.2 buck2 at `6507dd15`

- **The digest is computed client-side** in `prepare_action`, from argv, the
  *declared* env, the working directory, the input fingerprint, outputs, the
  timeout and the platform
  ([`command_executor.rs` L193-L250](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L193-L250)).
- **Platform properties are in the key even without remote execution.** For a
  local-only executor with `remote_cache_enabled = True`,
  `remote_execution_properties` are kept (defaulting to empty)
  ([`command_executor_config.rs` L364-L380](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L364-L380)).
  They then become the `platform` used for the digest
  ([`common.rs` L456](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_server/src/daemon/common.rs#L456)).
  A plain `Executor::Local` uses an empty platform, a no-op cache checker and
  a no-op uploader ([`common.rs` L260-L271](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_server/src/daemon/common.rs#L260-L271)).
  **This is turnkey's lever for salting keys** (§5, G3).
- **Symlinks that leave the project are keyed by their target string.**
  Metadata is read component by component from the project root. At the first
  symlink with an absolute target it stops and returns
  `ExternalSymlink(target, rest)`
  ([`io/fs.rs` L229-L262, L310-L361](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_common/src/io/fs.rs#L229-L262)).
  In the action's input tree this becomes one REAPI `SymlinkNode` at the link's
  location (for example `.turnkey/godeps`), pointing at the absolute target
  ([`directory.rs` L200-L208, L626-L665](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/directory.rs#L200-L208)).
  - So a whole dependency cell enters the key as the one string
    `/nix/store/<hash>-godeps-cell`, and its contents are never hashed or
    uploaded.
  - This corrects [test-undeclared-inputs.md §3.5](test-undeclared-inputs.md#35-the-turnkey-cells-which-form-enters-the-key-x3),
    which says a content digest of the resolved file enters the key. The
    source says otherwise. The practical effect is the same: the key changes
    when the cell's store path changes.
- **Local build actions inherit the daemon's whole environment**, except
  `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `LD_LIBRARY_PATH` and
  `LD_PRELOAD`
  ([`run.rs` L1289](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_action_impl/src/actions/impls/run.rs#L1289),
  [`environment_inheritance.rs` L107-L119](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/environment_inheritance.rs#L107-L119)).
  None of that environment is in the key.
  - No buckconfig setting clears it; the only other policies are the test
    allowlist and `empty()` (same file, L69-L130).
  - Tests get the cleared allowlist `PATH, USER, LOGNAME, HOME, TMPDIR,
    XDG_RUNTIME_DIR` (L19-L28).
- **Uploads of local build results need two gates.**
  - The executor needs `allow_cache_uploads = True` with the remote cache on
    ([`command_executor_config.rs` L313-L319](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L313-L319)).
  - The action needs `allow_cache_upload = True`, or
    `[buck2] default_allow_cache_upload = true`
    ([`run.rs` L1621-L1660](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_action_impl/src/actions/impls/run.rs#L1621-L1660),
    [`ctx.rs` L902-L907](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_server/src/ctx.rs#L902-L907)).
  - Only successful results are uploaded.
  - The prelude deliberately never uploads incremental Rust compiles or clippy,
    and otherwise defers to the default
    ([prelude `rust/build.bzl`](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/rust/build.bzl)).
- **Local test runs are never uploaded** ("We never upload local test
  executions",
  [`orchestrator.rs` L1531](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_test/src/orchestrator.rs#L1531)).
  This is unchanged since `7600cb80`, and it is why turnkey's runner records
  results itself (ADR 0001).
- **Auth.** `http_headers`, `tls_ca_certs` and `tls_client_cert` accept `$VAR`
  substitution
  ([`buck2_re_configuration/src/lib.rs` L418-L438](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_re_configuration/src/lib.rs#L418-L438)).
  A secret can therefore stay out of the Nix-generated `.buckconfig`: the
  config names `$VAR`, and the daemon's environment supplies it. buck2's
  BuildBuddy example uses exactly `http_headers = x-buildbuddy-api-key:$BUILDBUDDY_API_KEY`
  ([`examples/remote_execution/buildbuddy/.buckconfig`](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/examples/remote_execution/buildbuddy/.buckconfig)).

### 3.3 What turnkey's actions put in the key, and what they leave out

Read off the generated toolchains cell
(`.turnkey/toolchains/BUCK`, produced by
[`nix/buck2/mappings.nix`](../../nix/buck2/mappings.nix)) and the dev shell's
environment on aarch64-darwin:

| Input | In the key? | How | Cross-machine consequence |
|---|---|---|---|
| clang/clang++ (cxx), python3, node/tsc, solc/forge, jrsonnet, mdbook | **Yes** | absolute `/nix/store/…` strings in argv ([`mappings.nix` L84-L117, L226-L277, L480-L484](../../nix/buck2/mappings.nix#L84-L117)) | Same `flake.lock` and system → same key. A different system → different store hash → different key |
| Dependency cells (`godeps`, `rustdeps`, `pydeps`, `jsdeps`, `soldeps`), prelude, toolchains cell | **Yes**, as a `SymlinkNode` target | §3.2 | Same as above. Contents are never uploaded, so a worker needs the store path |
| Repo sources, buck-out artifacts | Yes, by content | input root | Project-relative, so checkout location doesn't matter ([test-undeclared-inputs.md §3.2](test-undeclared-inputs.md#32-working-directory-and-path-rendering)) |
| **`rustc`** (and rustdoc, clippy) | **No** | `system_rust_toolchain` hard-codes `RunInfo(args = ["rustc"])`, resolved from the inherited `PATH` ([prelude `toolchains/rust.bzl` L45](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/toolchains/rust.bzl#L45)). turnkey declares it without a compiler attribute ([`mappings.nix` L56-L73, L173-L190](../../nix/buck2/mappings.nix#L56-L73)) | **Wrong hit:** a branch that bumps `rust-toolchain` gets rlibs built by the old rustc. **RE:** fails, because workers get no `PATH` |
| **`go` / GOROOT** | **No** | `go_copy_goroot` runs `go env GOROOT` from `PATH` and copies the tree into buck-out ([`system_go_toolchain.bzl` L46](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/toolchains/go/system_go_toolchain.bzl#L46), [`go/tools/copy_goroot.go` L30](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/go/tools/copy_goroot.go#L30)). Its argv holds only the (built) copy tool | **Wrong hit, including across OSes:** the copy tool is a buck-built Go binary, so its digest differs per OS only if its own inputs do *(inferred; not traced)*. Downstream Go actions key on the copied GOROOT's content, so they are safe once this one action is right |
| Dev-shell env: `NIX_CFLAGS_COMPILE` (includes `-frandom-seed=…`), `NIX_LDFLAGS`, `NIX_HARDENING_ENABLE`, `SDKROOT`, `DEVELOPER_DIR`, `MACOSX_DEPLOYMENT_TARGET`, `CC`, `CXX`, `PYTHONPYCACHEPREFIX`, … | **No** | inherited by every build action (§3.2). Observed with `direnv exec . env` | Nixpkgs' clang wrapper reads the `NIX_*` flags *(inferred from how cc-wrapper works; not traced in this note)*, and clang reads `SDKROOT` and `MACOSX_DEPLOYMENT_TARGET`. Outputs then depend on the dev shell, not only on the key. **RE:** a worker's cleared environment produces *different* bytes under the *same* key |
| Genrule `bash`, test-time `PATH`/`HOME` | Tests: yes (pinned by `test_caching.bzl`). Genrules: no | [`test_caching.bzl`](../../nix/buck2/prelude-extensions/test_caching/test_caching.bzl) | Tests are covered; build-side scripts are not |
| OS / arch | **No** | Execution platform `re_properties` are empty | An action whose argv and inputs name no system-specific store path gets the same key on darwin and Linux |

**Takeaway.** Nix already gives turnkey most of the hermetic key that Bazel
users build with `rules_nixpkgs`. Two toolchains (Rust, Go) and the inherited
environment are the leaks. Each needs a deliberate fix before a shared cache is
enabled (§5).

## 4. Options

### 4a. Remote cache only, local execution (recommended first step)

- **Shape.** A turnkey-owned execution platform replaces
  `prelude//platforms:default`:
  `CommandExecutorConfig(local_enabled = True, remote_enabled = False,
  remote_cache_enabled = True, allow_cache_uploads = <writer?>,
  remote_execution_properties = {<salt>})`. Add `[buck2]
  default_allow_cache_upload = true` on writers only.
- **Every action still runs on the machine that asks.** A hit downloads the
  outputs instead of running the action. The worker-side /nix/store problem
  does not arise, because the machine that misses runs the action with its own
  store.
- **Trust.** A writer's key covers only what buck2 hashes. With the leaks in
  §3.3, a laptop that writes can poison CI. Let **only CI write**, and only
  from protected branches or a trusted workflow. This is also how the
  test-cache spec framed it ("who may write to a shared cache").
  `bazel-remote` supports the split natively: `--htpasswd_file` for writers
  and `--allow_unauthenticated_reads`
  ([bazel-remote v2.6.2 README L205-L226](https://github.com/buchgr/bazel-remote/blob/v2.6.2/README.md?plain=1#L205-L226)).
  Hosted services use read-only API keys (§4e).
- **What it buys in CI.** Unchanged Rust crates, Go packages and cxx objects are
  downloaded, not rebuilt, across PRs and runs, and across runner VMs, which
  have no persistent buck-out. Tests get the same through the runner (§6).
- **Laptops.** Linux laptops share keys with Linux CI when their `flake.lock`
  matches `main`. Mac laptops only benefit if a macOS CI job writes darwin
  entries (§4c).
- **Cost of a miss:** one AC lookup per action (a network round-trip). buck2
  also consults its in-memory local action cache first
  ([buck2-pinned-test-caching.md §3b](buck2-pinned-test-caching.md#2-test-result-cache-lookups-and-uploads-local-on-remote-off-remote-cache-on)).

### 4b. Full remote execution with Linux workers

What it adds over 4a:

- CI fan-out beyond 4 vCPUs.
- Laptops offload Linux actions.

What it needs:

1. **Every `/nix/store` path an action names must exist on the worker.** buck2
   uploads none of them (§3.2). Ways to provide them:
   - bake the toolchain closure and cells into the worker image (`container-image`
     built with `dockerTools`/`nix2container`);
   - mount a shared, read-only `/nix/store` into workers;
   - have workers substitute missing paths from Cachix on demand.

   Cells change with every dependency bump, so baking them into images doesn't
   scale. A worker that can realise store paths (nix-daemon plus a binary cache)
   does *(inferred)*. See [§4f](#4f-nix--remote-execution-prior-art).
2. **No reliance on `PATH`**: `rustc` and `go` must become store paths (G1, G2
   in §5). RE workers start from a cleared environment
   ([local-re-api-servers.md, NativeLink/Buildbarn env](local-re-api-servers.md#4-sandboxing-and-hermeticity)).
3. **Local and remote outputs must be byte-identical** for a hybrid setup to be
   coherent. That requires the scrubbed environment (G4).
4. **Server quirks.**
   - Buildbarn's scheduler ignores buck2's `Command.platform`
     ([buck2#1477](https://github.com/facebook/buck2/issues/1477)).
   - buck2 sends no `PATH`.
   - buck2 treats a dangling AC→CAS reference as a hard error
     ([buck2#862](https://github.com/facebook/buck2/issues/862)).

   All three are covered in [local-re-api-servers.md](local-re-api-servers.md).
5. **Tests on RE.** buck2 caches RE-executed tests natively. turnkey's runner
   would then not need to record them. But `go_test` and `python_test`
   render absolute paths unless `use_project_relative_paths` is set, which the
   caching helper already does
   ([test_caching.bzl](../../nix/buck2/prelude-extensions/test_caching/test_caching.bzl)).

### 4c. macOS

- **Keys never cross OSes, and shouldn't.** Store paths differ per system. The
  key salt (G3) must add `os`/`arch` so that actions without store paths
  cannot collide.
- **Cache-only on macOS works the same way as on Linux.** It needs a macOS CI
  writer:
  - GitHub's standard `macos-latest` is 3-core M1, 7 GB RAM, 14 GB SSD, and
    the repo has no macOS job
    ([parity-suite-ci-gate.md](parity-suite-ci-gate.md#the-runners)).
  - `turnkey-composed` does not build on macOS runners (same note), so the
    slim CI shell matters here too.
- **macOS RE workers** means Mac hardware you run yourself (BuildBuddy
  executors run natively on darwin; Buildbarn publishes darwin binaries) or a
  paid enterprise offering. No vendor hosts macOS RE workers out of the box (§4e).
  - No OSS option sandboxes a darwin worker
    ([local-re-api-servers.md](local-re-api-servers.md#facts-most-decisive-for-the-mechanism-choice-turnkey-w557)).
  - A plus for turnkey: Nix supplies the Apple SDK (`SDKROOT` is a store path
    in the dev shell), so workers wouldn't need a matching Xcode. That holds
    only once the SDK reaches actions through the key, not through inherited
    env (G4).
- **Recommendation:** macOS as a cache *reader* first. Whether a macOS CI
  writer is worth its runner minutes is an open question (§7).

### 4d. Roll our own

"Roll our own" here means operating an off-the-shelf REAPI cache, not writing
a server. Writing one is not justified: keys, AC/CAS semantics and buck2's
client are all standard, and good servers exist.

| Variant | What it is | Effort | Notes |
|---|---|---|---|
| **bazel-remote + object store** | One `bazel-remote` (already packaged in turnkey: `TURNKEY_TEST_CACHE_SERVER`, [`buck2.nix` L1159](../../nix/devenv/turnkey/buck2.nix#L1159)) on a small VM, or as a CI service, with `--s3.*`/`--gcs_proxy.*` backing, htpasswd for writers and anonymous reads ([README L205-L405](https://github.com/buchgr/bazel-remote/blob/v2.6.2/README.md?plain=1#L205-L405)) | Low. One service plus a bucket; TLS in front | Cache only. AC→CAS completeness checked by default. SHA256 only, which matches buck2's default |
| **Per-job bazel-remote on `actions/cache`** | Each CI job starts bazel-remote over a directory persisted by `actions/cache` | Lowest; no infrastructure | GitHub's cache budget applies: 10 GB per repo, 7-day idle eviction (G9). No laptop sharing |
| **Laptop tiering** | The per-user bazel-remote gets `--grpc_proxy.url` to the shared cache: read-through, with asynchronous write-through ([local-re-api-servers.md, bazel-remote tiering](local-re-api-servers.md#5-tiering)) | Low, once the shared cache exists | Keeps the local test cache. Laptops need a proxy mode that doesn't push, *(unverified)* whether bazel-remote can make a proxy read-only; otherwise use auth that rejects writes |
| **NativeLink or Buildbarn, self-hosted, with workers** | Full RE | High: workers need Nix (4b) | Only worth it after 4a proves the keys sound |

### 4e. Services

Shared and hosted terms only. Laptop-local behaviour of the same servers is in
[local-re-api-servers.md](local-re-api-servers.md#comparison). Prices and tiers
were read on 2026-09-25 and change often.

| Service | Cache only | RE | buck2 | macOS RE | Server-side "CI writes, others read" | Self-host / licence | Nix-friendliness |
|---|---|---|---|---|---|---|---|
| **BuildBuddy** cloud | Yes | Yes, Linux | buck2 ships an example with `http_headers = x-buildbuddy-api-key:$BUILDBUDDY_API_KEY` ([example](https://github.com/facebook/buck2/tree/2026-09-15/examples/remote_execution/buildbuddy)). BuildBuddy's own docs have no buck2 page | Docs: `darwin` is "only available for self-hosted executors" ([rbe-platforms.md](https://github.com/buildbuddy-io/buildbuddy/blob/v2.310.0/docs/rbe-platforms.md)). The pricing page lists Mac cores, which contradicts the docs *(unverified)* | Read-only keys and CAS-only keys ([api_keys.tsx L411-L453](https://github.com/buildbuddy-io/buildbuddy/blob/v2.310.0/enterprise/app/api_keys/api_keys.tsx#L411-L453)), plus roles | Free "Personal" tier: 100 GB cache transfer, 80 Linux cores, 10 users. Custom images need Team or above ([pricing](https://www.buildbuddy.io/pricing)). OSS core MIT; RE and executors enterprise | No Nix docs. Workers need a custom image with the closure baked in, or self-hosted executors with `docker_volumes` bind-mounting a host store ([docker.go L62](https://github.com/buildbuddy-io/buildbuddy/blob/329df6c808ad6fe7e56d1dd110cb959a7fbc2dd8/enterprise/server/remote_execution/containers/docker/docker.go#L62)) |
| **NativeLink** | Yes | Yes | Dedicated buck2 page ([buck2.mdx](https://github.com/TraceMachina/nativelink/blob/v1.7.1/web/apps/docs/content/docs/getting-started/connect-your-build/buck2.mdx)); buck2 example; CI-tested | Self-hosted workers run on darwin, with no isolation | AC `read_only: true` on a public endpoint and a writable CI endpoint; mTLS. No API keys ([servers-and-services.mdx](https://github.com/TraceMachina/nativelink/blob/v1.7.1/web/apps/docs/content/docs/configuration/servers-and-services.mdx)) | NativeLink Cloud is discontinued; managed means Enterprise ([oss-and-enterprise.mdx](https://github.com/TraceMachina/nativelink/blob/v1.7.1/web/apps/docs/content/docs/reference/oss-and-enterprise.mdx)). FSL-1.1; the always-on metrics module is BUSL, so production self-hosting "technically" needs an agreement (same page) | **Best prior art.** LRE builds worker images with nix2container, so that local and remote store paths, and therefore digests, match ([lre.mdx](https://github.com/TraceMachina/nativelink/blob/v1.7.1/web/apps/docs/content/docs/explanations/lre.mdx)). It is Bazel-only, "highly experimental", and parity holds only within one architecture |
| **EngFlow** | Yes | Yes | buck2 example (mTLS via `tls_client_cert`); EngFlow's own docs are Bazel-only | Enterprise only; "local execution only" (no sandbox) on macOS ([action-env](https://docs.engflow.com/re/config/action-env.html)) | `cache-reader` / `cache-writer` roles ([roles](https://docs.engflow.com/multitenancy/roles-policies-permissions.html)) | Free tier is self-hosted, one machine, 32 cores, Linux ([pricing](https://www.engflow.com/product/pricing)). Proprietary | `container-image` pinned by digest; no Nix docs |
| **Buildbarn** (self-host; Aspect Workflows is a managed option) | Yes | Yes | buck2 example; the scheduler ignores `Command.platform` ([buck2#1477](https://github.com/facebook/buck2/issues/1477)) | Self-hosted `bb_worker` on darwin via NFSv4, with no kext | Per-instance `putAuthorizer`, JWT, mTLS ([bb-storage README](https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/README.md)) | Apache-2.0 | Advertises absolute symlinks as `ALLOWED` ([ac_blob_access_creator.go L27](https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/configuration/ac_blob_access_creator.go#L27)). The rules_nixpkgs RE tutorial is Buildbarn plus an NFS-mounted `/nix/store` (below) |
| **bazel-remote** | Yes, only mode | No | No docs, but turnkey already runs it against the pinned buck2 for test caching (turnkey-w55.10, [feasibility note](test-cache-mechanism-feasibility.md#verification-run-turnkey-w5510-2026-09-25)); [buck2#1475](https://github.com/facebook/buck2/issues/1475) | n/a | htpasswd or mTLS for writes, plus `--allow_unauthenticated_reads` | Apache-2.0; in nixpkgs; S3/GCS/Azure backends | Irrelevant for cache-only: nothing runs on the server |
| **BuildFetch Cache** | Yes | No | Dedicated buck2 page ([docs](https://buildfetch.com/docs/buildfetch-cache/buck2-remote-cache)), which itself recommends read-only tokens and `allow_cache_uploads=false` on laptops | n/a | Read-only tokens | Free OSS plan 20 GB storage / 50 GB traffic ([pricing](https://buildfetch.com/cache-pricing)); proprietary | n/a |

Services that are **not** REAPI or don't support buck2: Depot Cache (lists
no buck2), FlakeHub Cache and Cachix (Nix store only), nixbuild.net and garnix
(Nix builders).

**No vendor offers hosted macOS RE out of the box.** Every option expects Mac
workers you run yourself, except BuildBuddy's pricing page, which contradicts
its docs.

### 4f. Nix + remote execution: prior art

- **Nobody publicly runs buck2 RE with `/nix/store` actions today.** Two
  patterns recur:
  - **Provide the same store paths on workers, outside REAPI.** NativeLink LRE
    bakes them into a nix2container image. rules_nixpkgs builds on a shared Nix
    server and NFS-mounts its store read-only on Buildbarn workers
    ([remote-execution tutorial](https://github.com/tweag/rules_nixpkgs/blob/v0.14.0/docs/remote-execution-tutorial.md),
    [nixpkgs.bzl L422-L455](https://github.com/tweag/rules_nixpkgs/blob/v0.14.0/core/nixpkgs.bzl#L422-L455)).
    Tweag's own blog names that approach's limits: storage growth with no GC
    signal, and NFS sync delays
    ([Tweag, 2024-02-29](https://www.tweag.io/blog/2024-02-29-remote-execution-rules-nixpkgs/)).
    rules_nixpkgs' CI excludes macOS from these jobs
    ([workflow.yaml L35-L40](https://github.com/tweag/rules_nixpkgs/blob/v0.14.0/.github/workflows/workflow.yaml#L35-L40)).
  - **Keep Nix steps local, share only the rest.** Mercury's snowydeer marks
    `nix_build` `local_only = True, allow_cache_upload = False` so that "store
    paths are made to exist", and keeps RE off in the public repo
    ([snowydeer](https://github.com/MercuryTechnologies/snowydeer)).
    tweag/buck2.nix does the same (`local_only = True`,
    [flake.bzl](https://github.com/tweag/buck2.nix/blob/038b031b84846101030b9d081445003e82e3be5c/flake.bzl)).
    thoughtpolice/buck2-nix, which ran Nix inside Buildbarn runners, is
    deprecated
    ([README](https://github.com/thoughtpolice/buck2-nix/blob/25667514dd90aaa30aaf433670f6aa5b29931293/README.md)).
- **Turnkey is already closest to the second pattern:** Nix realises every
  store path before buck2 runs (`nix develop` / cells). That is exactly why
  **cache-only works for turnkey without any worker provisioning**, and RE
  does not.
- **Absolute-symlink handling differs by server.**
  - The REAPI capability `symlink_absolute_path_strategy` lets a server refuse
    absolute targets
    ([proto L1126-L1139](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1126-L1139)).
  - NativeLink advertises `DISALLOWED` but its worker creates them anyway
    ([capabilities_server.rs L176](https://github.com/TraceMachina/nativelink/blob/v1.7.2/nativelink-service/src/capabilities_server.rs#L176),
    [running_actions_manager.rs L754-L758](https://github.com/TraceMachina/nativelink/blob/v1.7.2/nativelink-worker/src/running_actions_manager.rs#L754-L758)).
  - buck2 never reads the capability *(inferred from a grep)*.
  - This only matters for RE; a cache server never looks at input trees.
- **Provisioning techniques for RE workers**, from most to least proven:
  1. nix2container or `dockerTools` images selected with `container-image`.
     A full miss on every toolchain change, and images can't carry
     per-dependency-bump cells without rebuilding.
  2. A host `/nix/store` bind-mounted into worker containers and filled from
     Cachix.
  3. An NFS-shared store.
  4. nix-snapshotter
     ([architecture](https://github.com/pdtpartners/nix-snapshotter/blob/v0.4.0/docs/architecture.md))
     or snix FUSE stores: no RE deployment found.

  For turnkey, whose cells change with every dependency bump, (2) fits best
  *(inferred)*: a self-hosted Linux worker running nix-daemon, whose pre-action
  hook runs `nix-store --realise` on the paths named by a platform property.
  This is the idea from
  [rules_nixpkgs#180](https://github.com/tweag/rules_nixpkgs/issues/180#issuecomment-1329081409),
  and no server implements it off the shelf.

## 5. Gap analysis: what turnkey is missing

Ordered by dependency. Effort: **S** is under a day, **M** is days, **L** is a
week or more *(estimates)*.

| # | Gap | Why it matters | Effort |
|---|---|---|---|
| G0 | **CI doesn't run buck2.** It needs a slim CI shell without `turnkey-composed`, jj, beadwork or mdbook, the cells pushed to Cachix, and a `tk build //... && tk test //...` job (`turnkey-kxe` covers the parity-suite subset) | Without it there is no CI to speed up and no trusted writer | M |
| G1 | **Rust toolchain from `PATH`.** Give buck2 a store-path `rustc` (and rustdoc, clippy-driver): either a turnkey `system_rust_toolchain` variant or a prelude patch adding a `compiler` attr | Wrong hits across `rust-toolchain` bumps. RE impossible | S–M |
| G2 | **Go GOROOT from `PATH`.** Make GOROOT a keyed input: e.g. a cell symlink `goroot -> /nix/store/…-go/share/go` (keyed as a `SymlinkNode`), or pass the store path in `copy_goroot`'s argv | Wrong hits, including darwin↔Linux. RE impossible | S–M |
| G3 | **No key salt for environment, OS or arch.** Put `remote_execution_properties` on the turnkey execution platform: `{"turnkey-os": …, "turnkey-arch": …, "turnkey-env": <hash of the build env/toolchain closure>}`. They enter `Command.platform`, and so the key (§3.2) | Closes cross-OS collisions now, and any not-yet-found leak coarsely. Cost: a toolchain bump misses everything, which is correct | S |
| G4 | **Build actions inherit the dev-shell environment.** Have `tk` start the buck2 daemon with a minimal, pinned environment (store-path `PATH`, fixed `HOME`/`TMPDIR`, no `NIX_*`), since buck2 has no setting for it (§3.2). Where a toolchain needs a variable (`SDKROOT`, `MACOSX_DEPLOYMENT_TARGET`), pass it as declared action env through the toolchain, so it enters the key | Hidden inputs: wrong hits, and local≠remote bytes under RE. Also makes laptop and CI outputs identical | M: needs care, because `tk` is also the interactive wrapper; test cells, `tk sync`, etc. |
| G5 | **No turnkey-owned execution platform.** Replace `prelude//platforms:default` in the generated `.buckconfig` with a prelude extension that takes the cache flags and the salt from `buckconfig` (`turnkey.remote_cache`, `turnkey.cache_writer`) | Needed for anything in §4. Keeps a no-cache default | S |
| G6 | **No build-cache or auth options.** Generalise `buck2.testCache` into `buck2.remoteCache` (endpoint, `tls`, `instance_name`, `httpHeaders` with `$VAR`, a writer flag), and write `[buck2_re_client]` whenever a cache is configured, not only for tests | Hosted services need headers or mTLS. The writer must be CI only. Secrets stay in env | S |
| G7 | **Test runner can't write to a remote.** Add TLS and header support to the recorder, and replace the "is the host local" rule with an explicit writer setting (CI = writer) | A shared test cache, the "shared-cache handover" the spec deferred | S–M |
| G8 | **No local→shared tiering.** Configure the per-user bazel-remote with `--grpc_proxy.url` to the shared cache, rather than `endpoint` replacing it | Laptops keep fast local hits, and gain CI's | S |
| G9 | **Cells, toolchains cell and prelude not in Cachix, and Linux only.** CI's `magic-nix-cache-action@v8` depends on the GitHub Actions cache. Its free mode was shut down in 2025-02 and revived in 2025-06, and DetSys warns "the rug could in principle be pulled at any time" ([DetSys blog](https://determinate.systems/blog/bringing-back-magic-nix-cache-action/)). The `actions/cache` budget is 10 GB per repo by default, with 7-day idle eviction ([GitHub docs](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)). Push the CI shell's closure, including cells, to Cachix. Add `aarch64-darwin` if a macOS job exists | CI cold start is dominated by Nix realisation ([parity-suite-ci-gate.md](parity-suite-ci-gate.md#blockers-in-order-of-weight)). An action-cache hit is useless if the job first spends 20 min building cells | S–M |
| G10 | **Verification tooling.** A CI check that runs the same targets twice on different runners (or checkouts) and diffs the action digests (buck2 `log what-ran`/event log), plus an "is this output reproducible" spot check. The parity suite already compares digests ([parity-suite-ci-gate.md](parity-suite-ci-gate.md#what-the-suite-does)) | Catches new leaks before they poison the cache | M |
| G11 | **Remote execution workers** (only if wanted): a Nix-provisioned Linux worker image or a store-sharing worker, `container-image`/properties on the platform, hybrid config | CI parallelism beyond one runner | L |

Not gaps:

- **Checkout-location independence** is already handled: project-relative
  paths, `BUCK_ISOLATION_DIR=.turnkey`, and the test helper forcing
  project-relative rendering.
- **The digest function** is SHA256, buck2's default.
- **The buck2 version** is pinned by turnkey (ADR 0002). A bump can change
  test digests (`DefaultHasher`,
  [buck2-pinned-test-caching.md §1b](buck2-pinned-test-caching.md#1b-stable-deterministic-test-output-paths-are-the-default)),
  but that only causes misses.

## 6. Test result caching specifically

- **Already done, locally.** turnkey's runner records passes under buck2's own
  digest, and buck2 looks them up natively
  ([ADR 0001](../adr/0001-runner-recorded-native-test-caching.md)). Cached
  tests pin `PATH` to store paths and `HOME` to `/homeless-shelter`, and
  render project-relative paths
  ([`test_caching.bzl` L60-L79](../../nix/buck2/prelude-extensions/test_caching/test_caching.bzl#L60-L79)).
  Test keys are therefore already in better shape than build keys.
- **Test keys still inherit build-side leaks.** A test's inputs include its
  compiled binary. If that binary came from a wrong build-cache hit (G1, G2,
  G4), a stale test pass follows. So test sharing must come *after*, or
  together with, the build-key fixes.
- **Shared test cache = G7 + G6 + G3**:
  - CI writes passes to the shared endpoint.
  - Laptops read through their local tier (G8).
  - The `recorded, remote` marker and event-log `origin` already exist
    ([user manual](../user-manual/src/workflows/test-result-caching.md#a-remote-cache)).
- **With RE (4b)**, buck2 would cache RE-executed tests natively, and the
  runner's recording would become redundant for those. Local runs would still
  need it, because buck2 never uploads local tests (§3.2).
- **Bazel parity.** Only passes are shared, as in Bazel
  ([bazel-test-result-caching.md](bazel-test-result-caching.md#the-reuse-policy-in-eight-rules)).
  The spec's "only passes" rule carries over unchanged.
- **The unreachable-endpoint cost** (about 45 s of retries, then every opted-in
  test fails) matters more for a remote cache on flaky networks. `tk` can't
  probe a remote endpoint today (user manual, same section).

## 7. Suggested plan, CI first

1. **Make the keys honest (G1–G5).** No server is needed yet.
   - Verify with G10 on one Linux runner and one Linux laptop: same targets,
     same `main`, identical digests.
   - Do the same across darwin/Linux: disjoint digests.
2. **Run buck2 in CI (G0, G9).** Use the slim shell and push cells to Cachix.
   Measure the cold-to-warm Nix cost first. It may dominate everything else.
3. **Stand up a Linux cache, CI writes only (G6).** Either:
   - bazel-remote on S3/GCS behind TLS, or
   - a hosted cache with CI-only write keys.

   Enable `default_allow_cache_upload` on CI only. Measure the hit rate on PRs.
4. **Share test results (G7).** CI records passes; laptops read. Add laptop
   tiering (G8).
5. **macOS:** reader-only at first. Add a macOS CI writer if Mac laptop hit
   rates justify the runner minutes.
6. **Remote execution (G11):** only if CI wall time is still CPU-bound after
   steps 1–4.

### Open questions for the user

1. **Who may write?** CI on `main` only, or CI on every PR too (faster PR
   iterations, but PR code can write entries that `main` will read)? Laptops
   never?
2. **Hosted or self-hosted?** Budget, data-residency and ops appetite: a SaaS
   bill versus one bazel-remote plus a bucket.
3. **Is macOS CI in scope?** Without a macOS writer, Mac laptops get no shared
   hits.
4. **Does turnkey ship this to consumers** (module options, defaults), or only
   use it in its own CI? This decides whether G3–G6 are turnkey features or
   repo configuration.
5. **How coarse may the environment salt (G3) be?** Hashing the whole dev
   shell is simple and correct, but invalidates everything on any
   `flake.lock` change. Per-toolchain salts hit more often, at the cost of
   more wiring.
6. **Should `tk` own the daemon environment (G4)?** It is the most invasive
   change, and the one that makes RE and cross-machine reuse sound.

## Not covered or not verified

- Nothing was built or run against a remote service. Hit rates and
  download-vs-rebuild trade-offs are unmeasured.
- Whether nixpkgs' clang wrapper reads the unsuffixed `NIX_CFLAGS_COMPILE` /
  `NIX_LDFLAGS` from a dev-shell environment was inferred, not traced in the
  wrapper source.
- Whether `go_copy_goroot`'s digest actually differs between darwin and Linux
  (through the copy tool's own digest) was not traced. G3 closes it either way.
- How much buck2 materialises on a CI cache hit (deferred materialisation) was
  not checked.
