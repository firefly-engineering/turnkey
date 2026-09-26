# How remote-execution workers get Nix store paths on demand

Research for **turnkey-wit.3**, part of map turnkey-wit (build service). The
question: how can an executor make available, before it runs an action, the
`/nix/store` paths that the action names? And how have others done it?

The map has already settled the **programmable base image**: one generic
executor image. For each action, the store paths it names are fetched from the
service's Nix cache, cached on the instance and mounted read-only into the
action's sandbox. This note does not reopen that decision. It works out how
to carry it out, and lists the facts that bear on it (§8).

- **Researched:** 2026-09-26. Every fact below was read on that date unless
  a different date is given.
- **Pinned sources:**
  - buck2 [`6507dd15`](https://github.com/facebook/buck2/tree/6507dd157a6f81a810c48583edf1758dd0c337c5)
    (release `2026-09-15`, turnkey's pin) and prelude
    [`4d101dce`](https://github.com/facebook/buck2-prelude/tree/4d101dce3482c35b32f9f1e7072b354ae789d256).
  - Nix tag `2.35.2`, commit
    [`2c73b59d`](https://github.com/NixOS/nix/tree/2c73b59da29606068c0c98db015dd3a66955525d)
    (2026-08-12). The link prefix `nix@2c73b59d` below means that tree.
  - nixpkgs `f8573b9c` (turnkey's `flake.lock`).
  - Cloud Run docs showing "Last updated 2026-09-24".
  - bubblewrap `v0.13.0`.
- **Method.**
  - Source and docs reading.
  - `nix path-info` against turnkey's own cells on an aarch64-darwin checkout.
  - `nix path-info --store https://cache.nixos.org` for x86_64-linux
    toolchains at turnkey's nixpkgs pin.

  Nothing was deployed, and no fetch was timed. Statements not read off a
  source are marked *(inferred)*; statements that could not be checked are
  marked *(unverified)*.

This note builds on, and does not repeat:

- [remote-execution-and-caching.md](remote-execution-and-caching.md):
  - §3.2: external symlinks become `SymlinkNode`s keyed by their target string,
    and their contents are never uploaded.
  - §3.3: what turnkey's actions put in the key.
  - §4b, §4f: the first survey of prior art and provisioning techniques.
  - §5: gaps G1 (rustc from `PATH`), G2 (GOROOT from `PATH`) and G4
    (inherited environment).
- [local-re-api-servers.md](local-re-api-servers.md): buck2 sends platform
  properties only on `Command`, and each server's sandboxing.

## 1. Answer in brief

- **Discover the paths from the action itself, on the worker.** Scan three
  places for absolute store paths:
  - the input root's `SymlinkNode` targets;
  - `Command.arguments`;
  - `Command.environment_variables`.

  Then close each path over narinfo `References`. Everything this finds is
  already in the action key, because it *is* the action. So discovery adds
  nothing to the key and can never disagree with it.
- **Treat a platform-property list of closure roots as an optional extra, not
  the main channel.** In OSS buck2, platform properties belong to the
  execution platform, not the action (§3.2). They sit in *every* action's
  key, so a root such as the toolchains cell would re-key every action
  whenever any toolchain changes.
- **Fetch with a purpose-built fetcher, and run no nix-daemon.** The steps:
  1. BFS over narinfo files.
  2. Check each ed25519 signature against the service key.
  3. Stream-decompress and unpack each NAR into a per-instance store directory.
  4. Canonicalise the result, then rename it into place atomically.

  go-nix (Go) and snix's `nix-compat` (Rust) already parse and verify narinfo
  and read NARs. Unpacking to disk takes about 50 lines on top.
  `nix copy --to 'local?root=…'` is a working fallback. From the code, it
  should need no privileges *(inferred)*.
- **Cost of a cold fetch.**
  - Unpacked closures (NAR size): a Rust toolchain is about 1.7 GB, clang about
    1.9 GB, Go 0.24 GB.
  - Cells range from 0.1 MB (pydeps) to 0.55 GB (rustdeps).
  - With xz, decompression costs about 14 s of CPU per 1.7 GB. With zstd, it
    costs about 1.5 s.
  - The service's cache should therefore store zstd NARs.
  - A warm instance pays nothing: the store is cached per path, and paths are
    shared between closures. For example, `llvm-lib` (566 MB) is in both the
    clang and rustc closures.
- **Mount read-only in layers.**
  - **Always:** run the action under a different uid from the store's owner.
    The store keeps Nix's canonical 0444/0555 modes. This needs no mount
    privileges.
  - **Where mounts are available:** also give each action a mount namespace
    that bind-mounts **only its closure**, read-only, at `/nix/store`. This is
    how the Nix sandbox and nix-snapshotter do it.
  - **On Cloud Run:** the documented route is Cloud Run *sandboxes*
    (Preview), with `readonly` bind mounts. Raw `unshare`/`mount` inside a
    gen2 container is undocumented.
- **The logical path must be exactly `/nix/store`; the physical location
  need not be.** Keep the cache at, say, `/var/lib/tk-store/nix/store` (a Nix
  "chroot store" layout) and bind it at `/nix/store` inside the sandbox.
- **Skip lazy FUSE stores for now.** snix can serve a binary cache as a lazy
  FUSE `/nix/store`. But nothing documents a user FUSE daemon working inside
  a Cloud Run gen2 container today. The only evidence is a 2023 tutorial that
  Google has since replaced. An eager per-path cache on a warm instance
  already makes the common case free.

## 2. Prior art

Only two systems fetch store paths **on demand per job**: nixbuild.net's
private FUSE filesystem and snix's FUSE/virtiofs store. All the others do one
of two things:

- realise the whole closure up front, through nix-daemon or a substituter;
- share one store that was filled beforehand (NFS, or a container image).

No Bazel or buck2 remote-execution project was found that realises
`/nix/store` paths per action.

Short link names:

- NS = [nix-snapshotter `8e875fb8`](https://github.com/pdtpartners/nix-snapshotter/tree/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a) (v0.4.0)
- SX = [snix `e79bb1f1`](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864)
- NL = [NativeLink `b9d01f2c`](https://github.com/TraceMachina/nativelink/tree/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6) (v1.7.2)
- BB = [bb-remote-execution `c7bbd666`](https://github.com/buildbarn/bb-remote-execution/tree/c7bbd666d6dbf5a5f85258d7752f2f2254f58160)
- BBD = [BuildBuddy `266066a5`](https://github.com/buildbuddy-io/buildbuddy/tree/266066a57cb351ae38d5f5c71289ff94b2a49eb5)
- RN = [rules_nixpkgs v0.14.0](https://github.com/tweag/rules_nixpkgs/tree/9e67c2e1f947cf371c4445f83f1f197f99b14ac5)
- HC = [hercules-ci-agent 0.10.8](https://github.com/hercules-ci/hercules-ci-agent/tree/0120f2f3a5ccee32d81fd25036223e1dafa04d64)

| System | (a) How it finds the paths | (b) How it fetches them | (c) How the job sees them | Numbers |
|---|---|---|---|---|
| **nix-snapshotter** | Image layer annotations. `containerd.io/snapshot/nix-closure` points at a file listing the closure ([NS `pkg/nix2container/generate.go` L30-L46](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/pkg/nix2container/generate.go#L30-L46)) | `nix-store --add-root … --realise <path>` as a subprocess, when the image is pulled; not lazy. Can be swapped for an `external_builder` script ([`pkg/nix/nix.go` L51-L73](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/pkg/nix/nix.go#L51-L73)) | **One read-only `rbind` per store path**, plus an overlay for the writable root filesystem ([`pkg/nix/snapshotter.go` L337-L399](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/pkg/nix/snapshotter.go#L337-L399)). They chose binds over overlay lowerdirs because overlayfs allows at most 128 lower layers, and bind mounts have "no known limit" ([`docs/architecture.md` L72-L92](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/docs/architecture.md#L72-L92)) | None. Needs control of containerd; rootless support is "TODO" |
| **snix** | Per store path, on request | `nix+https://` path-info service: on `get`, it "fetches the .narinfo and referred NAR file" and ingests the **whole path** into snix's castore. That is "quite a costly operation", so a local cache goes in front ([SX `store/src/pathinfoservice/nix_http/mod.rs` L32-L46](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/store/src/pathinfoservice/nix_http/mod.rs#L32-L46)). Lazy *per file* only for content already in the castore. The binary-cache protocol "does not (yet) allow partial/lazy substitution", because the NAR hash covers the whole path | `snix-store mount` (FUSE) or `virtiofs`. snix-build mounts a FUSE daemon per build over the input root nodes ([SX `build/src/buildservice/oci.rs` L84-L100](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/build/src/buildservice/oci.rs#L84-L100)) | None. Docs warn of "known (and not yet worked-on) performance issues" |
| **nixbuild.net** | The derivation's inputs | Its own storage, binary caches, or client uploads ([blog 2020-08-13](https://blog.nixbuild.net/posts/2020-08-13-build-reuse-in-nixbuild-net.html)). Per the founder, "a FUSE file system that fetches content on demand" with "an index of the nar files to allow random access" ([Discourse, 2023-09-29](https://discourse.nixos.org/t/lazy-loading-of-store-paths/33638)). Not officially documented *(unverified)* | Virtualised sandbox holding only the inputs | CI job cut from 20 min to 20 s with a closure "just shy of 10 GB", by using it as a remote store ([blog 2022-03-16](https://blog.nixbuild.net/posts/2022-03-16-lightning-fast-ci-with-nixbuild-net.html)) |
| **Hercules CI agent** | The scheduler sends `inputDerivationOutputPaths` | `ensurePath` on each input through the Nix store or daemon; falls back to building them ([HC `…/Worker/Build.hs` L25-L52](https://github.com/hercules-ci/hercules-ci-agent/blob/0120f2f3a5ccee32d81fd25036223e1dafa04d64/hercules-ci-agent/hercules-ci-agent-worker/Hercules/Agent/Worker/Build.hs#L25-L52)) | Nix's own sandbox | None. Eager, whole closure |
| **garnix** | Not documented | Not documented *(unverified)* | | |
| **rules_nixpkgs** | `nixpkgs_package` at load time | With `BAZEL_NIX_REMOTE`: `nix build --store ssh-ng://host`, a GC root on the server, then `nix copy --from` back ([RN `core/nixpkgs.bzl` L422-L452](https://github.com/tweag/rules_nixpkgs/blob/9e67c2e1f947cf371c4445f83f1f197f99b14ac5/core/nixpkgs.bzl#L422-L452)) | Workers NFS-mount the server's `/nix/store` read-only ([tutorial](https://github.com/tweag/rules_nixpkgs/blob/9e67c2e1f947cf371c4445f83f1f197f99b14ac5/docs/remote-execution-tutorial.md)) | None |
| **NativeLink LRE** | The toolchain image is fixed per platform: `exec_properties = {"container-image": "docker://lre-cc:<nix-hash>"}` ([NL `local-remote-execution/generated-cc/config/BUILD` L36-L47](https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/local-remote-execution/generated-cc/config/BUILD#L36-L47)) | Baked into a nix2container image at build time | The image | x86_64-linux only; "highly experimental" |
| **omnibin** (not RE; 2026-09-24) | An index built from Hydra's `.ls` listings on cache.nixos.org | FUSE at `/omnibin`: it "lazily fetch[es] the NARs from the cache and unpack[s] them on-demand" | FUSE | First run 2.7 s, then instant ([blog](https://fzakaria.com/2026/09/24/every-package-is-already-installed)) |

**Where the design discussion stands** ([rules_nixpkgs#180](https://github.com/tweag/rules_nixpkgs/issues/180)):

- **The original proposal** (aherrmann, 2022-11-28,
  [comment](https://github.com/tweag/rules_nixpkgs/issues/180#issuecomment-1329081409)):
  - Nix-imported targets carry platform properties naming their store paths.
  - Bazel would accumulate those properties transitively per action.
  - The executor realises them before running.

  It needs a Bazel feature that does not exist. buck2 has the same gap (§3.2).
- **BuildBuddy's position.** sluongng called per-action realisation the
  "cheapest and easiest-to-implement" option and offered to build it
  ([comment](https://github.com/tweag/rules_nixpkgs/issues/180#issuecomment-1381627032)).
  No sign it shipped *(unverified)*.
- **The objection to one global image.** It loses per-package granularity,
  and it breaks concurrent branches that pin different Nix inputs
  ([comment](https://github.com/tweag/rules_nixpkgs/issues/180#issuecomment-1438226096)).
  This is the argument for the settled "programmable base image".
- **Relocatable packs** (guix pack, nix-portable) do not help: shared libraries
  keep a RUNPATH into `/nix/store`
  ([comment](https://github.com/tweag/rules_nixpkgs/issues/180#issuecomment-2182361700)).

**REAPI worker hooks, in case an off-the-shelf worker is used**
(turnkey-wit.2 picks the component):

- **NativeLink.**
  - `entrypoint` is prefixed to every action's argv, so it can realise paths
    and then `exec` the action
    ([NL `nativelink-config/src/cas_server.rs` L1050-L1056](https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/cas_server.rs#L1050-L1056)).
  - `additional_environment` can map platform properties to env vars
    (L1094-L1099).
  - `experimental_precondition_script` runs *before* inputs are downloaded,
    so it is the wrong place for this
    ([`local_worker.rs` L129-L134](https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-worker/src/local_worker.rs#L129-L134)).
- **Buildbarn.**
  - bb_runner has no per-action pre-execution hook. It has
    `chroot_into_input_root` and `run_command_cleaner`, which runs at
    idle/busy transitions
    ([BB `bb_runner.proto` L36-L93](https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_runner/bb_runner.proto#L36-L93)).
  - The extension point is the Runner gRPC protocol: `Run(RunRequest)` carries
    argv, env and the input root directory
    ([`runner.proto` L22-L60](https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/runner/runner.proto#L22-L60)).
  - A custom runner is exactly where D2 + F3 + M3 would live *(inferred)*.
- **BuildBuddy.**
  - Has no Nix support.
  - The `run-under` platform property prefixes a wrapper to argv
    ([BBD `server/util/platform/platform.go` L126-L136](https://github.com/buildbuddy-io/buildbuddy/blob/266066a57cb351ae38d5f5c71289ff94b2a49eb5/server/util/platform/platform.go#L126-L136)).
    It is a platform property, and so part of the key.

**Nix's local overlay store** (experimental since Nix 2.22):

- It stacks a writable upper layer over a read-only lower store with
  OverlayFS. "Nix does not manage the overlayfs mount point itself."
  ([manual 2.28](https://nix.dev/manual/nix/2.28/store/types/experimental-local-overlay-store.html))
- It is for adding paths on top of a shared store. Actions never add store
  paths, so the executor does not need it (§6, M5).

## 3. What a turnkey action names, and where

### 3.1 The channels

| Channel | What reaches the action this way | Visible to the executor? | In the action key? |
|---|---|---|---|
| **Input-root `SymlinkNode` target** | Dependency cells (`godeps`, `rustdeps`, `pydeps`, `jsdeps`, `soldeps`), the prelude and the toolchains cell. Each is one symlink at `.turnkey/<cell>` ([`buck2.nix` L130-L175](../../nix/devenv/turnkey/buck2.nix#L130-L175)); buck2 stops at the first absolute symlink ([remote-execution-and-caching.md §3.2](remote-execution-and-caching.md#32-buck2-at-6507dd15)) | Yes: it is a field in the input tree | Yes, by target string |
| **`Command.arguments`** | Toolchain executables: clang, python3, node, solc, jrsonnet, … ([`mappings.nix` L129-L132](../../nix/buck2/mappings.nix#L129-L132)). When the prelude moves arguments into an argfile, **the executable stays in argv**: `_long_command` returns `cmd_args(exe, at_argfile(...))` for rustc/rustdoc ([prelude `rust/build.bzl` L1887-L1899](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/rust/build.bzl#L1887-L1899)), and cxx appends `@argsfile` after the compiler ([`cxx/compile.bzl` L2139-L2144](https://github.com/facebook/buck2-prelude/blob/4d101dce3482c35b32f9f1e7072b354ae789d256/cxx/compile.bzl#L2139-L2144)) | Yes | Yes |
| **`Command.environment_variables`** | The action's *declared* env only. A store-path `PATH` or `SDKROOT` would arrive here once G4 is fixed | Yes | Yes |
| **Argfile and script contents** | Flags such as `-isystem /nix/store/…` inside `@argsfile`s, and shebangs of scripts that buck2 writes into buck-out *(inferred)* | Only by reading input file contents | Yes, by content digest |
| **Inherited `PATH`** | `rustc` (G1), `go`/GOROOT (G2), and the `clang`/`lld` "needed in PATH" runtime deps ([`mappings.nix` L331-L339](../../nix/buck2/mappings.nix#L331-L339)) | **No.** buck2 sends no `PATH` to RE, and a worker starts from a cleared environment | **No**: this is the key leak in remote-execution-and-caching.md §3.3 |

Two consequences:

- **The last row is not a discovery problem.** A tool found through an
  inherited `PATH` is outside the key. It is a correctness leak, to be fixed
  (G1, G2, G4) before remote execution is enabled at all. Once fixed, those
  tools appear in argv or declared env, and the scan finds them. Until then,
  such an action fails on the executor with "command not found". That is
  better than a silently wrong result.
- **Argv alone misses paths that appear only inside argfiles.** In practice
  those are almost always inside the closure of the executable in argv. A
  clang wrapper's include and library paths, for example, are in
  `clang-wrapper`'s `References` *(inferred; not traced per flag)*. Scanning
  file contents in the input root is a possible extra channel (§4).

### 3.2 Platform properties are per execution platform in OSS buck2

- **The platform comes from the executor config.**
  - `CommandExecutorConfig(remote_execution_properties = {...})` sets it for an
    execution platform
    ([`command_executor_config.rs` L158, L201-L215](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L158)).
  - `re_create_action` copies that one platform into `Command.platform`
    ([`command_executor.rs` L236-L340](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L236-L340)).
- **The per-action channels are Meta-only.**
  - `ctx.actions.run` accepts `remote_execution_dependencies` and
    `remote_execution_dynamic_image`
    ([`context/run.rs` L183-L195, L555-L566](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_action_impl/src/context/run.rs#L183-L195)).
  - Their docs describe Meta's internal scheduler ("SMC tier", "Tupperware
    image").
  - The custom image is attached only under `#[cfg(fbcode_build)]`
    ([`command_executor.rs` L408-L420](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L408-L420)).
  - The OSS gRPC client never sends the dependencies: they appear only in the
    request struct
    ([`re_grpc/src/request.rs` L152-L157](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/request.rs#L152-L157)).
    This is based on a grep of `client.rs` *(inferred)*.
- **So "closure roots in a platform property" can only mean a fixed set per
  execution platform.** turnkey could define several execution platforms,
  but a target picks one; an action cannot pick its own roots. The value is
  also in every action's key
  ([proto L934-L960](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L934-L960)).
- **Per-action roots would need declared env.** An env var set by the rule,
  such as `TURNKEY_NIX_ROOTS=/nix/store/…`, is per-action and keyed. That
  requires prelude changes per rule; the scan in §4 gets the same paths
  without them.

### 3.3 Sizes (turnkey's own inputs)

**Linux toolchains**, x86_64-linux, nixpkgs `f8573b9c`, from
`cache.nixos.org`. "Unpacked" is the NAR size, i.e. what lands on disk.

| Root | Paths in closure | Unpacked | Compressed download | Biggest members |
|---|---|---|---|---|
| `clang-wrapper-21.1.8` | 34 | 1893 MB | 269 MB (all xz) | clang-lib 853 MB, llvm-lib 566 MB, gcc 277 MB |
| `rustc-wrapper-1.93.0` | 13 | 1655 MB | 329 MB | rustc 1041 MB, llvm-lib 566 MB |
| `go-1.25.7` | 9 | 239 MB | 46 MB | go 201 MB |
| `python3-3.13.12` | 22 | 183 MB | 59 MB | |
| `nodejs-24.13.0` | 57 | 248 MB | 51 MB | |

**turnkey's cells**, from the aarch64-darwin checkout. The Linux equivalents
should be similar in size *(inferred)*. Each cell is **a single store path with
no references** (`nix-store -q --references` returns nothing), so its closure is
itself:

| Cell | Unpacked |
|---|---|
| `rustdeps-cell` | 552 MB |
| `turnkey-prelude` | 17.9 MB |
| `godeps-cell` | 12.7 MB |
| `pydeps-cell` | 0.14 MB |

- **The toolchains cell** is 4.3 KB. It has **8 references, and they are
  exactly the 8 distinct store paths written in its `BUCK` file**. Nix's
  reference scan of the cell therefore already computes the set of toolchain
  executables that end up in argv. On darwin its closure is 133 paths and
  2.5 GB.
- **Toolchain bumps are rare; dependency-cell bumps are frequent.** Toolchains
  are shared between closures, and a warm instance fetches each one once. The
  frequent case is a new `rustdeps-cell` of about 0.5 GB after a
  dependency bump: one path, fetched once per instance.

## 4. Discovering the needed paths

| Option | How | Complete? | Key impact | Cost | Verdict |
|---|---|---|---|---|---|
| **D1. Symlink targets only** | Collect every `SymlinkNode` target under `/nix/store` in the input tree | No. It misses every toolchain in argv (clang, python, …) | None | Walk the tree the worker walks anyway | Not enough alone |
| **D2. D1 + argv + declared env** | D1, plus parse every argv element and env value for `/nix/store/<32 nix32 chars>-<name>` store-path prefixes | Yes, for everything the key names outside file contents. `PATH`-resolved tools are the exception, and they are outside the key anyway (§3.1) | None: every path found is already in the key | Microseconds per action | **Recommended primary** |
| **D3. D2 + input file contents** | Scan input files for store-path strings while materialising them, as Nix's reference scanner does | Catches argfile-only and shebang-only paths | None | Reads every input byte. Needs a size cap or a text-only filter *(inferred)*. Also has false positives: a path string that is not a real dependency triggers a fetch or a lookup failure | Optional fallback. Use only if D2 proves incomplete in practice |
| **D4. Platform property with closure roots** | `remote_execution_properties = {"nix-roots": "<toolchains cell path> …"}` | As complete as the roots chosen. The toolchains cell's closure covers all argv toolchains (§3.3) | **Every action on the platform re-keys when any root changes**, e.g. a clang bump re-keys all Go actions. Per platform, not per action (§3.2) | None at run time | Escape hatch for paths D2 cannot see; not the main channel |
| **D5. Per-action env var** | Rules add `TURNKEY_NIX_ROOTS` to declared env | Exact | Keyed per action, and only for that action | Prelude patch per rule | Unnecessary once D2 exists |

**Closure completion.**

- A discovered path is a *root*. The executor fetches its narinfo and follows
  `References` breadth-first.
- `References` lists **direct** references as store-path base names, space
  separated
  ([nix@2c73b59d `src/libstore/nar-info.cc` L77-L82](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/nar-info.cc#L77-L82)).
  There is one narinfo per path, at `<hashpart>.narinfo`
  ([`binary-cache-store.cc` L125-L128](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/binary-cache-store.cc#L125-L128)).
- So closure discovery costs one round-trip per BFS level, with the requests
  in each level issued in parallel *(inferred)*. Paths already on the instance
  are known to be closure-complete (below), so a warm BFS stops at once.

**Parse, don't pattern-match.**

- Nix's own scanner is a search for a *known* set of hashes: it checks
  `hashes.erase(ref)` against candidates
  ([nix `references.cc` L15-L35 at tag 2.31.2](https://github.com/NixOS/nix/blob/2.31.2/src/libstore/references.cc#L15-L35)).
- The executor has no candidate set. It must recognise the prefix
  `/nix/store/`, then a 32-character hash in the nix32 alphabet, then `-` and
  a name. Then it looks the hash part up in the cache.
- Store paths also occur inside larger strings: `/nix/store/<h>-clang/bin/clang`,
  `-I/nix/store/…/include`, `--sysroot=/nix/store/…`. The scan must find each
  `/nix/store/` occurrence, then hand the rest to a real store-path parser.
  The name ends at the first character outside Nix's name alphabet or at `/`.
- Existing parsers:
  - snix's `StorePath::from_absolute_path_full` splits a store path from a
    trailing sub-path
    ([`nix-compat/src/store_path/mod.rs` L111-L122 at `e79bb1f1`](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/nix-compat/src/store_path/mod.rs#L111-L122)).
  - go-nix's `storepath.FromAbsolutePath` expects exactly a store path
    ([`storepath.go` L80-L90 at `4bdde671`](https://github.com/nix-community/go-nix/blob/4bdde671e0a123e7ac8df2bbd2bcbb2f316f5f55/pkg/storepath/storepath.go#L80-L90)),
    so the caller does the splitting.

  A regex is not the tool for this.

**A path that is not in the cache.**

- The narinfo lookup returns 404, and the executor has only a store path,
  which cannot be built. Building needs the derivation. The deriver is named
  in the narinfo's `Deriver` field, which does not exist for a path the cache
  lacks
  ([`nar-info.cc` L50-L100](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/nar-info.cc#L50-L100)).
- The executor must therefore fail the action with a precondition error that
  names the missing paths, so the client can submit the derivations (the
  turnkey-wit.5 channel) and retry. It cannot "build it" on its own (§8).

## 5. Fetching

### 5.1 Options

| Option | What it is | Needs | Fit |
|---|---|---|---|
| **F1. `nix copy --from <cache> --to 'local?root=/var/lib/tk-store' <paths>`** | Nix itself, as a single-user "chroot store". The logical store stays `/nix/store`; the physical store is `<root>/nix/store`; the database is `<root>/nix/var/nix/db` ([nix@2c73b59d `src/libstore/local-store.md` L10-L30](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.md#L10-L30)) | Nix in the image. Namespaces are needed only for *building* or *running* from a chroot store. Substitution goes through `addToStore`: signature check → restore → hash check → canonicalise → optimise ([`local-store.cc` L1046-L1135](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.cc#L1046-L1135)). `build-users-group` setup runs only as root, and the default is empty (L167-L180). So it should work **unprivileged** *(inferred from code; not run on Cloud Run)* | Quickest to prototype. Brings a SQLite DB, Nix's settings surface and its single-threaded substitution decompression ([NixOS/nix#12355](https://github.com/NixOS/nix/issues/12355)) |
| **F2. nix-daemon in the container** | Multi-user Nix | Root, build users; the daemon's sandbox would need namespaces | Pointless: the executor never builds. The Nix *builder* is a separate component |
| **F3. Purpose-built fetcher** | narinfo BFS → verify `Sig` → stream NAR → decompress → unpack → canonicalise → atomic `rename` into the store dir | A library: [go-nix `4bdde671`](https://github.com/nix-community/go-nix/tree/4bdde671e0a123e7ac8df2bbd2bcbb2f316f5f55/pkg) has `narinfo.Parse`, `Fingerprint()`, ed25519 `signature.VerifyFirst` and a streaming `nar.NewReader`. snix [`nix-compat`](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/nix-compat/src/) has the same in Rust. **Neither ships an unpack-to-disk helper**, so we write about 50 lines, plus decompression (zstd/xz libraries) | **Recommended.** No DB, no daemon. Parallel per path and per core. Verifies the service's signature itself. It also becomes the executor's single seam to the Nix cache |
| **F4. `nix-store --restore`** | Unpacks a NAR from stdin | No DB, **no signature check** ([`nix-store.cc` L754-L762](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/nix/nix-store/nix-store.cc#L754-L762)) | Only as the unpack step inside F3 when shelling out, with verification done by the caller |

**What F3 must replicate from Nix.**

- **Signatures.** The fingerprint is
  `1;/nix/store/<path>;<narHash>;<narSize>;<comma-joined full reference paths>`
  ([`path-info.cc` L49-L56](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/path-info.cc#L49-L56)).
  Check it against the **service's own key only**. Under the trust rule, only
  the service's builder signs paths.
- **Hashes.** Check `NarHash` and `NarSize` while streaming.
- **Metadata.** Canonicalise as Nix does: mtime 1, modes 0444/0555, no
  xattrs or ACLs, owner = the store's uid
  ([`posix-fs-canonicalise.cc` L13-L33, L60-L81, L104-L110](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/posix-fs-canonicalise.cc#L13-L33)).
  NARs carry only one executable bit and no ownership.

### 5.2 Latency

- **Decompression.** On one core of an EPYC 9554, xz decompresses at about
  123–127 MB/s and zstd at about 1.07–1.37 GB/s (lzbench, Silesia corpus,
  [README L287-L310 at `817efafd`](https://github.com/inikep/lzbench/blob/817efafdb0021a0de903c6d06d7a665610a1320c/README.md#L287-L310)).
  For a 1.7 GB rustc closure, that is about 14 CPU-seconds with xz and about
  1.5 with zstd *(inferred arithmetic)*.
- **Compression on cache.nixos.org is mixed.** Every path in the three closures
  measured at turnkey's nixpkgs pin is xz. A sample of 25 narinfos per channel
  on 2026-09-26 found nixos-unstable and 26.05 already all zstd. There was no
  announcement *(unverified date)*.
- **The service controls its own cache format.** Store zstd (Nix 2.35 writes
  independent 16 MiB frames that a parallel decoder can split,
  [`compression.cc` L337-L368](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libutil/compression.cc#L337-L368)).
  A path the service substitutes from upstream in xz can be recompressed on
  ingest *(inferred)*.
- **Network.**
  - A Rust toolchain plus rustdeps is roughly 0.5–0.9 GB compressed: 329 MB
    for the toolchain, plus the rustdeps cell at its (unmeasured) compression
    ratio.
  - Cloud Run to GCS in-region bandwidth per instance was not measured and is
    not documented as a number here *(unverified)*.
  - At an assumed 1–2 Gbit/s, the transfer takes about 3–7 s *(inferred)*.
- **Round-trips.** One per BFS level: rustc's closure is 13 paths, clang's 34,
  in a handful of levels *(inferred)*.
- **Estimate.** On a cold instance, the first Rust action waits roughly 5–15 s
  with zstd, and several times longer with single-threaded xz *(inferred)*. On
  a warm instance it waits nothing, apart from new cells.
- **No published end-to-end substitution benchmark** was found from Nix or
  Determinate Systems.
- **A community measurement shows the cost of xz.** In 2022, downloads from
  cache.nixos.org ran at about 12 MB/s, bound by xz CPU
  ([Discourse, 2022-12-12](https://discourse.nixos.org/t/switch-cache-nixos-org-to-zstd-to-fix-slow-nixos-updates-nix-downloads/23961)).

### 5.3 Per-instance cache and eviction

- **Where the cache lives on Cloud Run.**
  - The container filesystem and in-memory volumes **count against instance
    memory**. The maximum is 32 GiB, and 8 vCPU needs 4–32 GiB
    ([container contract](https://docs.cloud.google.com/run/docs/container-contract),
    [in-memory volumes](https://docs.cloud.google.com/run/docs/configuring/services/in-memory-volume-mounts),
    [memory limits](https://docs.cloud.google.com/run/docs/configuring/services/memory-limits)).
    A Rust and clang toolchain plus rustdeps is about 3.5 GB of RAM before the
    action runs *(inferred from §3.3; the two toolchains share llvm-lib)*.
  - **Ephemeral disk (Preview, release note 2026-04-20)** is the better home.
    It is ext4, 1–100 GiB per volume (10 GiB per instance by default),
    deleted at instance shutdown, and billed for size × instance lifetime
    ([ephemeral disk](https://docs.cloud.google.com/run/docs/configuring/services/ephemeral-disk)).
    It fits "idle costs only stored bytes", because it disappears with the
    instance *(inferred)*.
- **Rules for the cache** *(inferred design, not prior art)*:
  - **Unit = one store path.** Closures overlap heavily (llvm-lib in both
    clang and rustc), so caching per closure would duplicate bytes.
  - **Invariant: a path is present only if all its references are present.**
    Fetch bottom-up: references first, then an atomic `rename` from a
    temporary directory into the store. A warm lookup is then one `stat` per
    root.
  - **Single-flight per path.** Concurrent actions that need the same path
    wait on one fetch.
  - **Eviction is LRU by last use.**
    - Pin paths in the closure of a running action.
    - Evict only paths no present path refers to, i.e. top-down. That keeps
      the invariant.
    - The high/low watermark is a fraction of the ephemeral disk, or of
      memory if the store lives in RAM.
  - **Scale to zero wipes the cache.** Every cold instance starts empty. A
    min-instances setting or a pre-warmed image layer would trade idle cost
    for latency. That is the Cloud Run ticket's (turnkey-wit.1) question,
    not this one.

## 6. Mounting read-only into the action sandbox

**The logical path must be exactly `/nix/store`.**

- A chroot store's logical store dir stays `/nix/store`
  ([`local-store.md` L14-L17, L32-L36](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.md#L14-L17)).
- Binaries embed absolute `/nix/store` paths: RPATH, ELF interpreter,
  shebangs.
- Signatures cover the absolute path (§5.1).
- The action's own `SymlinkNode`s and argv name `/nix/store/…` too.

**The physical location is free.** Keep the cache at
`/var/lib/tk-store/nix/store` and make it appear at `/nix/store` only inside
the sandbox. The executor's own image may be Nix-built and have a
`/nix/store` of its own. A per-action mount namespace hides that store from
the action, and the action needs none of it *(inferred)*.

| Option | Mechanism | Prevents writes? | Hides unrelated paths? | Privileges on Cloud Run gen2 | Notes |
|---|---|---|---|---|---|
| **M1. uid separation** | Store owned by the executor's uid, with canonical 0444/0555 modes. The action runs as a different, non-zero uid | Yes: chmod needs owner or `CAP_FOWNER` ([chmod(2)](https://man7.org/linux/man-pages/man2/chmod.2.html)), and writes need `CAP_DAC_OVERRIDE` ([capabilities(7)](https://man7.org/linux/man-pages/man7/capabilities.7.html)). 0555 directories stop unlink and rename *(inferred)* | No | None beyond `setuid()` from the executor process, which runs as root in the container's user namespace ([container contract](https://docs.cloud.google.com/run/docs/container-contract)). The contract's "no setuid binaries for non-root users" is about setuid *files*, not the syscall *(inferred)* | **Baseline, always on.** The action must never run as uid 0, which holds `CAP_DAC_OVERRIDE` inside the container's namespace. Needs the physical store to appear at `/nix/store` some other way: a symlink `/nix/store → /var/lib/tk-store/nix/store`, or put the store there directly |
| **M2. Whole-store read-only bind in a mount namespace** | `bwrap --ro-bind /var/lib/tk-store/nix/store /nix/store …` ([bubblewrap v0.13.0 `bwrap.xml`](https://github.com/containers/bubblewrap/blob/v0.13.0/bwrap.xml)); `MS_BIND|MS_REC`, then `mount_setattr(MOUNT_ATTR_RDONLY, AT_RECURSIVE)` (Linux ≥ 5.12) or a per-submount remount ([`bind-mount.c`](https://github.com/containers/bubblewrap/blob/v0.13.0/bind-mount.c), [mount_setattr(2)](https://man7.org/linux/man-pages/man2/mount_setattr.2.html)) | Yes. Once a less privileged namespace inherits it, `MS_RDONLY` is **locked** ([mount_namespaces(7)](https://man7.org/linux/man-pages/man7/mount_namespaces.7.html)) | No | Needs `unshare(CLONE_NEWUSER|CLONE_NEWNS)` and `mount`. gen2 claims "all system calls, namespaces" ([execution environments](https://docs.cloud.google.com/run/docs/about-execution-environments)), but the contract applies "basic security profiles, similar to seccomp" and forbids in-container mounting of *network* filesystems. Whether nested user namespaces and bind mounts work is **undocumented (unverified)** | Defer to the Cloud Run ticket (turnkey-wit.1) |
| **M3. Closure-only binds** | As M2, but bind **each path of the action's closure** read-only onto an empty `/nix/store` | Yes | **Yes.** An undiscovered dependency fails loudly instead of silently reading a path that happens to be cached, which keeps local and remote behaviour the same | As M2 | Prior art: the Nix sandbox binds each input-closure path individually ([`chroot-derivation-builder.cc` L52-L55](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/unix/build/chroot-derivation-builder.cc#L52-L55), [`linux-derivation-builder.cc` L246-L262](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/linux/build/linux-derivation-builder.cc#L246-L262)). nix-snapshotter does one read-only bind per closure path ([`snapshotter.go` L342-L343](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/pkg/nix/snapshotter.go#L342-L343)). Hundreds of binds per action are cheap *(inferred)* |
| **M4. Cloud Run sandboxes (Preview)** | `/usr/local/gcp/bin/sandbox do --mount type=bind,source=…,destination=/nix/store,readonly …` ([code execution](https://docs.cloud.google.com/run/docs/code-execution); announced 2026-07-08, extended to jobs and worker pools 2026-08-05) | Yes: writes return "Read-only file system". The root filesystem is read-only unless `--write` adds a tmpfs overlay | Yes, if each closure path is passed as its own `--mount` *(inferred; the doc does not say how many mounts are allowed)* | Documented, gen2. "All sandboxes are completely isolated from each other." Processes run as a non-root user. The isolation technology is not stated | **The documented route on Cloud Run.** Pre-GA. Throughput and per-call overhead unmeasured |
| **M5. Overlay** (`local-overlay://` store, or an overlayfs with the store as the read-only lower layer) | A writable upper layer over a read-only lower store | Lower layer: yes | No | overlayfs in a user namespace needs Linux ≥ 5.11 ([user_namespaces(7)](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)) | Solves a different problem (letting a Nix *build* add paths). Actions never add store paths, so this is not needed |

**Also available:** gVisor `runsc` run inside a gen2 service is a
Google-published sample, "not an officially supported Cloud Run feature"
([diy-sample-sandbox-cloud-run](https://github.com/GoogleCloudPlatform/diy-sample-sandbox-cloud-run)).
It gives full per-action containers with their own mounts, at the cost of
running a runtime inside the executor.

## 7. Lazy alternatives

| Option | What it would do | Status for us |
|---|---|---|
| **snix FUSE store** | `snix-store mount` serves a `/nix/store` over FUSE. Its `nix_http` path-info service fetches narinfo lazily from a plain binary cache and enforces `trusted_public_keys` ([`pathinfoservice/nix_http/mod.rs` L254-L260](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/store/src/pathinfoservice/nix_http/mod.rs#L254-L260), [`cli/store/src/main.rs` L119-L124, L595-L624](https://git.snix.dev/snix/snix/src/commit/e79bb1f11d54828e00e5b1794fd6dd4210984864/snix/cli/store/src/main.rs#L119-L124)) | The closest match to "on demand". Needs `/dev/fuse` and a FUSE mount inside the container. On gen2 that is **undocumented**: the contract forbids "manipulating devices" and setuid helpers such as `gcsfuse`. The only in-container FUSE evidence is a 2023 tutorial ([archive](https://web.archive.org/web/20231214095806/https://cloud.google.com/run/docs/tutorials/network-filesystems-fuse)), now replaced by managed volumes. gVisor (gen1) supports FUSE upstream ([gvisor.dev](https://gvisor.dev/docs/user_guide/fuse/)), but Cloud Run's gVisor configuration is not public *(unverified)* |
| **nixbuild.net-style indexed NAR FUSE** | FUSE store with an index of NAR offsets for random access; fetches file content on demand (§2) | Proven in production per its founder *(unverified: no docs or source)*. Same FUSE-on-Cloud-Run blocker as snix, and we would have to build it |
| **nix-snapshotter** | A containerd snapshotter that realises an image's store paths through `nix-store --realise` and binds them read-only ([`pkg/nix/nix.go` L55-L64](https://github.com/pdtpartners/nix-snapshotter/blob/8e875fb8eeb28947e2b4c3e7552f511d8d1f405a/pkg/nix/nix.go#L55-L64)) | Needs control of containerd. Not possible on Cloud Run; relevant only to a self-hosted Kubernetes executor |
| **Cloud Storage FUSE volume** holding an unpacked store | A managed read-only mount | Not POSIX-complete; performance "impacted by network bandwidth"; 200 MiB-chunk file cache ([Cloud Storage volumes](https://docs.cloud.google.com/run/docs/configuring/services/cloud-storage-volume-mounts)). Nix's modes and symlinks through GCS FUSE are *(unverified)*. It would also double storage, because the cache holds NARs |
| **NFS volume** (Filestore) of a shared store, as in rules_nixpkgs' Buildbarn tutorial | A shared read-only store | Supported with `readOnly` ([NFS volumes](https://docs.cloud.google.com/run/docs/configuring/services/nfs-volume-mounts)). But Filestore bills provisioned capacity while idle, which breaks the scale-to-zero requirement *(inferred)*. Tweag reports GC and sync problems (remote-execution-and-caching.md §4f) |

**Verdict.** Lazy fetching only pays off when actions read a small part of a
large closure. Warm instances make eager fetching free. The riskiest piece is
FUSE on Cloud Run, and it is the piece that is undocumented. Revisit if cold
starts dominate, or if the Cloud Run ticket (turnkey-wit.1) finds `/dev/fuse`
usable.

## 8. Options compared, and recommendation

| Combination | Cold latency (Rust action) | Warm latency | Complexity | Security |
|---|---|---|---|---|
| **R. D2 + F3 + M1, plus M3 or M4** | About 5–15 s with zstd *(inferred)* | ≈ 0 (stat per root; binds) | Medium: fetcher, cache and mount setup, about 1–2 kLOC *(inferred)* | Service-key signatures only. The action cannot write the store (M1), and sees only its closure (M3/M4) |
| D2 + F1 (`nix copy`) + M1 | Longer with xz and single-threaded decompression | ≈ 0 | Low: shell out to Nix | Same as R for writes. The Nix DB is writable by the executor and must stay out of the action's reach |
| D4 roots + F3 + M2 | As R | ≈ 0 | Medium | As R, but every toolchain bump re-keys every action |
| snix FUSE lazy store | Per-file lazy reads; first-byte latency on every miss | Low once cached | High: FUSE on Cloud Run unverified | Signature-checked by snix |
| Image with baked closures (NativeLink LRE style) | Image pull | ≈ 0 | Low per image, but an image rebuild per toolchain or cell change | Strong, but contradicts the settled generic image |

**Recommendation: R.**

- **Discovery (D2).** Symlink targets plus argv plus declared env, closed over
  narinfo `References`. D4 is optional and reserved for paths D2 cannot see.
  D3 is kept in reserve.
- **Fetching (F3).** A purpose-built, signature-verifying fetcher into a
  per-path, closure-complete cache on ephemeral disk. The service's cache
  stores zstd.
- **Isolation.** Actions always run as a non-root uid other than the store's
  owner (M1). On top of that, closure-only read-only binds at `/nix/store`,
  through Cloud Run sandboxes (M4) or namespaces (M3), whichever the Cloud
  Run ticket (turnkey-wit.1) confirms.
- **Missing paths** fail the action with a precondition error that lists them.
  The client submits the derivations and retries.

**Facts that bear on the map's settled decisions:**

- **"Closure roots named in a platform property."** Platform properties are
  per execution platform in OSS buck2, not per action (§3.2), and they are in
  every key. As the *main* channel, a root like the toolchains cell would
  re-key every action on any toolchain bump. Worker-side discovery (D2) finds
  the same paths at no key cost. A platform property is still useful for the
  few paths that exist only inside file contents. This refines the decision;
  it does not overturn it.
- **"Built by the service if missing."** The executor sees only an output
  path. It cannot recover the derivation from a path the cache lacks: the
  narinfo, and with it `Deriver`, is exactly what is missing. Building on a
  miss needs the client to have submitted the derivation first
  (turnkey-wit.5), or an error-and-retry protocol.
- **"Mounted read-only into the action's sandbox"** is achievable. On Cloud
  Run, the documented mechanism is the Preview sandboxes feature. Raw
  namespaces are undocumented, and uid separation (M1) works regardless.
- **The programmable base image itself is not contradicted.** Nothing found
  makes it infeasible. The per-instance cache belongs on ephemeral disk
  (Preview) rather than in RAM.

## 9. Not covered or not verified

- **Nothing was fetched or timed.** All latency figures are arithmetic on
  published throughput numbers and measured closure sizes.
- **Unprivileged `nix copy` into a `local?root=` store** was inferred from
  code, not run.
- **Whether gen2 permits nested user and mount namespaces, `/dev/fuse`, or
  bubblewrap** is undocumented. This belongs to the Cloud Run ticket
  (turnkey-wit.1).
- **Cloud Run sandboxes'** mount count limits, per-call overhead and
  isolation technology are not documented.
- **Cloud Run ↔ GCS bandwidth per instance** was not found as a number.
- **Linux sizes of turnkey's cells** were not measured; the darwin numbers
  stand in.
- **When cache.nixos.org switched to zstd** is not documented; the date is
  inferred from sampling.
- **Whether argfile-only store paths are always within the argv
  executable's closure** was not traced flag by flag.
