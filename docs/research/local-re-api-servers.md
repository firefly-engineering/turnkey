# Remote-execution-API servers on a developer machine

Research for **turnkey-w55.2**, part of epic turnkey-w55 (test result caching).
Gathered on 2026-09-25 from each project's own source, docs and releases, from
nixpkgs, and from buck2. Every claim links to the file and lines it rests on.
The last section lists the pinned revisions.

The question is which servers speaking the Bazel remote-execution API (REAPI)
could run on every developer machine, darwin and Linux, as a per-user
service. Each server is judged for two shapes:

1. **Local workers.** Tests execute on localhost workers behind a local
   server. buck2 serves a cached test result only when a remote-execution
   worker produced it, so this is the shape that makes buck2's own test
   execution caching work.
2. **Cache-only.** An action cache (AC) and content-addressable store (CAS),
   filled by buck2 uploading the results of locally run *build* actions
   (`allow_cache_uploads`). buck2 never uploads a local *test* run
   ([source][b2-test-no-upload]).

buck2's client adds constraints of its own that apply to every server: TCP
only, SHA256, and platform properties sent only on `Command`. They are
collected in
[What buck2 requires of any server](#what-buck2-requires-of-any-server).

## Comparison

### Can it do the job?

Each cell is sourced in the per-server sections below.

| Server (version read) | Cache-only | Local workers on the same machine | Workers native on darwin | buck2 example / known issue | AC hit only if outputs are still in CAS ([buck2#862][b2-862]) | Local store in front of a shared one | Licence |
|---|---|---|---|---|---|---|---|
| **NativeLink** 1.7.1 | Yes | Yes, in one process | Yes, with no isolation. Release binary for aarch64 only; CI only checks that it starts | buck2 example, and NativeLink's own CI runs buck2. Needs `instance_name` configured | Opt-in (`completeness_checking`). The stock examples leave it out | `fast_slow` + `grpc` store; digests unchanged | FSL-1.1-Apache-2.0 (source-available) |
| **bazel-remote** 2.6.2 | Yes; this is its only mode | No | n/a | No example; used with buck2 in the wild ([buck2#1475][b2-1475]) | Yes, on by default | gRPC proxy (called experimental), HTTP, S3, GCS, Azure | Apache-2.0 |
| **Buildbarn** (bb-storage `b0032b1`, bb-remote-execution `c7bbd66`) | Yes (`bb_storage`) | Yes, 4–5 processes (`bare` deployment) | Yes. darwin binaries for every commit; no sandbox | buck2 example (docker-compose). The scheduler ignores buck2's platform properties ([buck2#1477][b2-1477]) | Opt-in (`completenessChecking`). The `bare` config uses it | `readCaching`, `readFallback`, `grpc`; `bb_clientd` | Apache-2.0 |
| **buildbox-casd** 1.4.26 (BuildGrid project) | Yes | Yes (`--buildbox-run`), no scheduler | casd and the no-isolation runner build on macOS via Homebrew. Upstream CI and nixpkgs are Linux-only. Local execution on darwin is unverified | None. Binds a Unix socket by default | Yes | `--cas-remote`, `--ac-remote`, `--exec-remote`, `--read-only-remote` | Apache-2.0 |
| **BuildBuddy** OSS 2.310.0 | Yes | Enterprise licence only | Enterprise executor only | Example against the hosted service | Yes | Enterprise-only Cache Proxy | MIT (OSS) / BuildBuddy Enterprise License |
| BuildGrid 0.8.11 | Yes | Through `buildbox-worker`; needs PostgreSQL | Not documented | None | Yes | Yes | Apache-2.0 |
| Bazel Buildfarm 2.17.1 | Only with Redis plus a shard worker | Yes | Not documented | README lists buck2 as a client | Not checked | Not checked | Apache-2.0 |
| justbuild `just execute` 1.6.6 | **No**: rejects client AC writes | Yes | **No**: Linux only | None; needs `--compatible` | Yes, by design | No | Apache-2.0 |
| EngFlow | Only the MyEngFlow Mini Docker image, x86_64 Linux | Only in the paid product | Only in the paid product | Example against the hosted service | — | `--cas_fallback_cluster` (paid) | Proprietary |

### Can it live on a laptop?

| Server | nixpkgs (pinned `f8573b9c` / master) | Other distribution | Processes and config | Listens on | Storage limit and eviction | Survives restart | Service units upstream |
|---|---|---|---|---|---|---|---|
| **NativeLink** | Not packaged | Its own flake, 4 systems; building from source needs 15–20 GB. Release binaries for linux-musl x86_64 and aarch64, and aarch64-darwin (44 MB) | 1 binary, JSON5 | TCP only; examples bind `0.0.0.0` | Per store, LRU by `max_bytes`, `evict_bytes`, `max_seconds`, `max_count` | Yes, filesystem store; rescans the directory at startup | systemd system units in the docs; no launchd |
| **bazel-remote** | `bazel-remote` 2.6.1 / 2.6.2, darwin and Linux, prebuilt in cache.nixos.org (22 MB NAR on aarch64-darwin) | Docker images | 1 binary; flags, env or YAML | TCP or Unix socket | `--max_size` (GiB, required), LRU | Yes; scans the directory at startup | systemd *system* unit example |
| **Buildbarn** | Not packaged | darwin, Linux, FreeBSD and Windows binaries for every commit | Cache-only: 1 binary. Workers: 4–5 processes. Jsonnet configs | TCP or Unix socket | Fixed-size block files, FIFO rotation with refresh-on-read | Yes, with `persistent.stateDirectoryPath` | `bb_clientd`: systemd *user* unit |
| **buildbox-casd** | `buildbox` 1.3.54 / 1.4.26, **Linux only** | Homebrew `recc` formula: macOS ≥ Sonoma arm64 bottles, with a launchd service | 1 binary, flags only | TCP or Unix socket (Unix by default) | `--quota-high` / `--quota-low` LRU, off by default | Yes | Homebrew launchd service |
| **BuildBuddy** OSS | Not packaged | Release binaries for linux amd64/arm64 and darwin **amd64 only**, about 94 MB | 1 binary plus SQLite plus web UI; YAML | TCP only; binds `0.0.0.0` on 4 ports | `cache.max_size_bytes` (10 GB default), LRU by atime | Yes | None for the OSS server |

## Per-server details

### NativeLink

Read at tag v1.7.1 (commit `612fff0b`, released 2026-09-17)
([release][nl-release]).

**1. Shapes and darwin**

- **Cache-only works.** `schedulers` and `workers` are optional in the top-level
  config ([source][nl-top-config]). A capabilities service with no
  `remote_execution` block reports that execution is unsupported
  ([source][nl-caps-no-re]). That is enough for buck2, which only needs
  `GetCapabilities` to answer at `engine_address` (see below).
- **Local workers work in one process.** `basic_cas.json5` runs CAS, AC,
  scheduler and a `local` worker together ([source][nl-basic-cas]). The docs
  call this "Shape 1: one process" ([docs][nl-docs-local-testing]).
- **The darwin worker is native.** Namespace isolation defaults to off
  "precisely so it runs unprivileged and on macOS" ([docs][nl-docs-namespaces]).
- **darwin support is thin.**
  - The only darwin release binary is aarch64 ([release][nl-release]).
  - CI only checks that this binary prints its usage line
    ([release workflow][nl-release-ci]).
  - All functional CI, including the buck2 integration test, runs on Ubuntu
    ([nix workflow][nl-nix-ci], [cargo workflow][nl-cargo-ci]).

**2. buck2**

- **buck2 ships an example.** It is in
  [`examples/remote_execution/nativelink`][b2-ex-nativelink]:
  `grpc://localhost:50051` for all three endpoints, `tls = false`,
  `instance_name = main`. Its README still installs v0.6.0
  ([README][b2-ex-nativelink-readme]). buck2's Linux CI builds this example
  against a hosted NativeLink ([workflow][b2-ci]).
- **NativeLink tests buck2 itself.** Its CI runs a buck2 build against a local
  single-process server. The buck2 side uses `tls = false`,
  `instance_name = main`, empty `remote_execution_properties` and
  `remote_output_paths = "output_paths"`
  ([.buckconfig][nl-buck2-buckconfig], [platform][nl-buck2-platform]).
- **The instance name must be configured on the server.** Otherwise the
  request fails with "'instance_name' not configured"
  ([source][nl-ac-instance]).
- **Platform-property matching.** For each property the scheduler uses one
  rule ([source][nl-props]):
  - `minimum`: numeric lower bound.
  - `exact`: the values must be equal.
  - `priority`: passed through without restricting.
  - `ignore`: accepted but never required.
- **`basic_cas` does not restrict on `OSFamily` or `container-image`.** It
  marks both as `priority` ([source][nl-basic-cas-props]). A
  `container-image = docker://…` sent by buck2 therefore does not stop the
  local worker from running the action directly on the host. (Inference from
  the config.)
- **buck2#862 applies.** buck2 assumes an AC hit never points at an evicted CAS
  blob ([buck2#862][b2-862]). NativeLink enforces this only when the AC is
  wrapped in a `completeness_checking` store ([schema][nl-completeness],
  [docs][nl-docs-compose]). Neither `basic_cas.json5` nor the buck2 test
  config does this: both put a plain filesystem store under the AC
  ([basic_cas][nl-basic-cas], [buck2_cas][nl-buck2-cas]).

**3. Packaging, footprint, storage**

- **Not in nixpkgs.** It is absent at turnkey's pinned `f8573b9c` and on master.
- **Its own flake builds packages for x86_64/aarch64 Linux and darwin**
  ([flake systems][nl-flake-systems], [packages][nl-flake-packages]). The
  quickstart's `nix run` builds from source and asks for 15–20 GB free
  ([docs][nl-docs-quickstart]).
- **The aarch64-darwin release binary is a single 44 MB Mach-O file**
  (17 MB tarball). This was measured by unpacking the release; the binary was
  not run. Memory use and startup time are not documented.
- **Stores.** Memory, filesystem, `fast_slow`, compression, dedup,
  `existence_cache`, `completeness_checking`, verify, shard, `grpc`, Redis,
  S3/GCS/Azure and others ([schema][nl-stores]).
- **Eviction is LRU.** The limits are `max_bytes`, `evict_bytes`,
  `max_seconds` and `max_count` ([schema][nl-eviction]). A memory store with no
  policy grows without bound ([schema][nl-memory-unbounded]).
- **The filesystem store persists across restarts**
  ([schema][nl-fs-persistent]). At startup it stats every file to rebuild its
  LRU, so startup time grows with the cache ([source][nl-fs-startup]).
- **The examples keep their data under `/tmp`** ([source][nl-basic-cas]).

**4. Sandboxing and hermeticity**

- **Inputs.** Each action gets its own work directory built from its input
  tree, hardlinked from the worker's filesystem fast store
  ([source][nl-worker-hardlink]).
- **Environment.** The worker clears the environment, then applies
  `additional_environment`, then the action's own variables. No `PATH`, `HOME`
  or `TMPDIR` is added ([source][nl-worker-env],
  [docs][nl-docs-worker-exec]).
- **Isolation.** Actions run as the worker's user and can read the whole host
  filesystem and use the network.
  - The optional Linux namespaces are "hygiene… not a security boundary"
    ([docs][nl-docs-worker-exec-ns], [source][nl-ns]).
  - On darwin there is no isolation at all, and `use_namespaces` makes the
    worker exit at startup ([source][nl-darwin-ns]).
- **Caching of results.** By default workers write a result to the AC only when
  the exit code is 0 (`SuccessOnly`) ([source][nl-success-only]). For tests,
  that means failures are never cached.

**5. Cache keys and tiering**

- **Keys are the client's action digest, unchanged.** `instance_name` only
  selects which store serves a request ([source][nl-ac-key]).
- **Hash functions.** SHA256 (the default) and BLAKE3 ([source][nl-hashers]).
- **Tiering uses a `fast_slow` store.** Reads fall back to the slow tier and
  fill the fast one, and uploads go to both. Each direction can be limited to
  `update`, `get` or `read_only` ([schema][nl-fast-slow]).
- **A `grpc` store forwards to another REAPI server** with the same digest,
  rewriting only the instance name ([source][nl-grpc-store]).
- **The docs show the combination:** a local filesystem store in front of a
  remote `grpc` store, for both CAS and AC. They say that digests must match,
  and call this "the one incompatibility that cannot be worked around"
  ([docs][nl-docs-migrate]).
- **Caveat: a `grpc` store under an AC is described as "likely will work"**
  ([schema][nl-grpc-ac-caveat]).

**6. As a per-user service**

- **One binary, one argument:** `nativelink <config>`. The config is JSON5 with
  `${VAR}` expansion ([docs][nl-docs-cli]).
- **Service units.** "No systemd unit exists in the NativeLink repository";
  the docs give system-level units to copy
  ([docs][nl-docs-bare-metal]). There is no launchd plist and no NixOS or
  nix-darwin module.
- **TCP only.** The examples bind `0.0.0.0:50051` (clients) and
  `0.0.0.0:50061` (worker API) ([listener schema][nl-listener],
  [example][nl-basic-cas-listen]).
- **AC writes are accepted unless the AC is `read_only`**, which returns
  `PermissionDenied` ([schema][nl-read-only], [source][nl-ac-read-only]).
  buck2 reads that as "uploads disabled".
- **Licence.** FSL-1.1-Apache-2.0 since 2025-09: source-available, turning into
  Apache-2.0 two years after each release ([LICENSE][nl-license]). The docs
  place the metrics and persistent-worker modules under BSL 1.1
  ([docs][nl-docs-oss-enterprise]).

### bazel-remote

Read at v2.6.2 (commit `3cb3084b`, released 2026-07-23)
([releases][br-releases]).

**1. Shapes**

- **Cache only.** It serves ActionCache, CAS, ByteStream and Capabilities, and
  never registers Execution ([source][br-services]).
- **Capabilities** ([source][br-caps]):
  - digest functions: SHA256 only
  - AC updates: enabled
  - batch size: `MaxBatchTotalSizeBytes = 0` ("no limit")
  - compression: zstd
  - no execution capabilities
- **buck2's reading of those capabilities.** It turns 0 into its own default of
  4,000,000 bytes ([source][b2-batch]) and picks zstd for ByteStream
  ([source][b2-bytestream-zstd]).

**2. buck2**

- **No buck2 example or doc.** It is not in `examples/remote_execution/` and
  buck2's docs don't mention it.
- **It is used with buck2 in practice.** buck2#1475 (2026-08) reports
  bazel-remote 2.6.2 as its remote cache. The bug there is that the OSS client
  does not retry an h2-wrapped connection reset ([buck2#1475][b2-1475]).
- **Config it needs:**
  - `address = grpc://127.0.0.1:9092` (or all three endpoint keys)
  - `tls = false`
  - `[buck2] digest_algorithms = SHA256`
  - The default gRPC port is 9092 ([flags][br-flags-grpc]).
- **Instance names are ignored** by Capabilities and ByteStream. The AC is
  namespaced by instance only with `--enable_ac_key_instance_mangling`
  ([README][br-readme-mangling], [source][br-ac-mangling]).

**3. Packaging, footprint, storage**

- **nixpkgs**:
  - `bazel-remote` 2.6.1 at turnkey's pinned nixpkgs
    ([package.nix][np-bazel-remote]); 2.6.2 on master
  - platforms: `darwin ++ linux`
  - prebuilt in cache.nixos.org (2026-09-25 query): aarch64-darwin output is
    a 22.4 MB NAR (25.5 MB closure); x86_64-linux is 24.4 MB (60.2 MB closure)
- **Single Go binary.**
- **Disk:**
  - `--dir` and `--max_size` (GiB) are required.
  - Eviction is LRU.
  - `--max_size_hard_limit` rejects writes over the limit.

  ([flags][br-readme-size], [LRU][br-readme-lru], [hard limit][br-readme-hard-limit])
- **CAS blobs are stored zstd-compressed by default**
  ([README][br-readme-storage-mode]).
- **Restarts:** the whole directory is scanned and sorted by atime at startup,
  "so that the eviction behavior is preserved across server restarts"
  ([source][br-load]). The scan finishes before the listeners start
  ([source][br-main-order]), so startup time grows with file count. No figures
  are published.
- **Memory:**
  - No RAM guidance.
  - The systemd example only suggests `GOMEMLIMIT` and `LimitNOFILE=40000`
    ([example unit][br-systemd]).
  - The LRU index keeps one entry per file in memory ([source][br-lru]).

**4. Integrity**

- **The AC→CAS check is on by default.** `GetValidatedActionResult` answers
  "not found" if any referenced output, tree, stdout or stderr blob is missing
  ([source][br-ac-validate]). It can be disabled with
  `--disable_grpc_ac_deps_check`.
- **Client AC writes are accepted** ([source][br-ac-update]).
  - Auth is optional: htpasswd, mTLS or LDAP, plus
    `--allow_unauthenticated_reads` ([README][br-readme-auth]).

**5. Tiering**

- **One proxy backend at a time:** HTTP, gRPC (called experimental in the
  help), GCS, S3 or Azure ([README][br-readme-proxy]).
- **Reads fall through.** A local miss reads through to the proxy and is stored
  locally.
- **Writes are queued asynchronously** to the proxy, and are dropped when the
  queue is full ([source][br-disk-proxy], [source][br-grpcproxy-queue]).
- **Keys pass through unchanged.** The gRPC proxy forwards the client's digest
  as-is ([source][br-grpcproxy-digest]).
- **The backend must accept the default instance.** The proxy sends no
  instance name. At startup it requires the backend to advertise SHA256 and
  AC updates ([source][br-grpcproxy-caps]).
- **Known cost:** existence checks for blobs that live only in the proxy are
  not cached, so they are repeated on every build
  ([bazel-remote#919][br-919]).

**6. As a per-user service**

- **Configuration** comes from flags, `BAZEL_REMOTE_*` environment variables
  or YAML ([README][br-readme-config]).
- **Listeners** accept `unix://path.sock` ([flags][br-flags-grpc]), but buck2
  needs TCP.
- **`--idle_timeout`** makes the server exit after a period with no requests
  ([flags][br-flags-idle]).
- **Only a system-level systemd unit is shipped** (`User=bazel-remote`, or
  `DynamicUser`). There is no user unit, no launchd plist and no socket
  activation ([example unit][br-systemd]).
- **Batch size risk.** gRPC-go's default 4 MiB receive limit applies. buck2's
  4,000,000-byte batches plus per-message overhead could exceed it, as they
  did against Bazel's remote worker in buck2#583. The remedy then is a lower
  `max_total_batch_size` ([buck2#583][b2-583]). This is inferred and has not
  been observed with bazel-remote.

### Buildbarn

There are no releases. Read at bb-storage `b0032b1` (2026-09-24),
bb-remote-execution `c7bbd66` (2026-09-23), bb-deployments `a354856`,
bb-clientd `85d2ce6` and bb-adrs `00164e0`.

**1. Shapes and darwin**

- **Cache-only.** `bb_storage` alone serves CAS, AC, ByteStream and
  Capabilities. It reports AC updates as allowed or not according to its
  `putAuthorizer` ([source][bbs-main]).
- **Local workers without Docker.** bb-deployments' `bare/` deployment runs
  every process directly and "is known to work on FreeBSD, Linux and macOS". It
  is launched with `bazel run //bare:bare`, which builds from source
  ([README][bbd-bare-readme]). It runs:
  - a frontend `bb_storage` with schedulers
  - a storage `bb_storage`
  - `bb_scheduler`
  - `bb_worker` with a native build directory
  - `bb_runner` behind a Unix socket

  ([worker config][bbd-bare-worker], [runner config][bbd-bare-runner])
- **darwin is a first-class target.**
  - Every main-branch commit publishes darwin amd64 and arm64 binaries of
    `bb_storage`, `bb_scheduler`, `bb_worker`, `bb_runner` and `bb_clientd`
    ([bb-remote-execution release][bbr-release],
    [bb-storage release][bbs-release]).
  - On macOS the lazy-loading virtual filesystem is NFSv4, not FUSE. It is
    "supported on macOS starting with Sequoia 15.0" and needs no kernel
    extension ([config][bbr-nfsv4], [source][bbr-nfsv4-darwin],
    [ADR 0009][bba-0009]).
  - FUSE is compiled out on darwin ([source][bbr-fuse-disabled]). The native
    hardlinking build directory needs no mount at all
    ([config][bbr-worker-builddir]).
  - Unverified: whether a non-root user may make the NFSv4 mount.

**2. buck2**

- **buck2 ships an example**, [`examples/remote_execution/buildbarn`][b2-ex-buildbarn].
  It uses bb-deployments' docker-compose setup, the `fuse` or `hardlinking`
  instance name, and a `PATH` added to the runner. buck2 sends no `PATH`, and
  Buildbarn passes none through ([README][b2-ex-buildbarn-readme]).
- **Blocking incompatibility: buck2#1477.**
  - The scheduler reads platform properties only from `Action.platform`. It
    "does not fall back to reading platform properties from the Command
    message" ([source][bbr-action-key]).
  - The fallback option was removed in bb-remote-execution PR #202
    ([config][bbr-sched-proto], [PR][bbr-pr-202]).
  - buck2 sets the platform only on `Command` ([source][b2-command-platform]).
    Recent schedulers therefore see an empty platform and answer "No workers
    exist for instance name prefix … platform {}"
    ([buck2#1477][b2-1477]). The fix, [buck2#1366][b2-1366], is open.
- **Workarounds (inferred from the config, untested):**
  - Register the worker with an empty platform, which the `bare` config
    already does ([worker config][bbd-bare-worker]). buck2's properties then
    match it, because the scheduler sees none.
  - Or switch the scheduler's `platformKeyExtractor` from `action` (the
    `bare` setting) to `static`, which ignores what the client sends
    ([schema][bbr-sched-proto], [bare scheduler][bbd-bare-scheduler]).
- **Required output mode.** PR #202 also dropped REv2.0 output fields. The
  worker reads only `Command.output_paths` ([source][bbr-output-paths]). That
  matches buck2's OSS default `remote_output_paths = "output_paths"`
  ([source][b2-output-paths-default]).
- **Digest functions.** SHA256 and BLAKE3, among others
  ([source][bbs-digest-functions]).

**3. Footprint and storage**

- **Memory:** not documented.
- **The `local` backend** keeps fixed-size block files plus a key-location map,
  created with `ftruncate` and `mmap` ([source][bbs-blockdevice]).
  - Sizes: 32 GiB of CAS blocks in `bare` ([config][bbd-bare-storage]) and
    100 GiB in `bb_clientd` ([config][bbc-config-size]).
  - Eviction is FIFO block rotation. Reading data in an old block copies it
    forward, which approximates LRU ([schema][bbs-local-schema]).
  - Data survives restarts with `persistent.stateDirectoryPath`
    ([schema][bbs-persistent]).
- **`completenessChecking`** returns an ActionResult only when all its outputs
  are still in the CAS ([schema][bbs-completeness]). The `bare` deployment
  wraps its AC with it ([config][bbd-bare-common]).

**4. Sandboxing and hermeticity**

- **Inputs.** Each action's build directory holds only its input root,
  hardlinked (native) or loaded lazily (FUSE or NFSv4)
  ([config][bbr-worker-builddir]).
- **Environment.** The command gets the action's variables, plus any
  `environment_variables` configured on the worker (the action wins), plus
  optionally `TMPDIR`. Nothing is inherited from `bb_runner`
  ([source][bbr-runner-env], [config][bbr-worker-env]).
- **No isolation primitives.**
  - There are no namespaces and no `sandbox-exec`, so actions can read any
    absolute path, `/nix/store` included.
  - `chroot_into_input_root` exists ([config][bbr-runner-chroot]).
  - `run_commands_as` requires `bb_runner` to run as root
    ([config][bbr-runner-runas]).
- **darwin-specific runner options** exist: Xcode developer directories and
  Mach-O architecture handling ([config][bbr-runner-xcode]).

**5. Cache keys and tiering**

- **Keys are plain REv2 digests.**
  - CAS keys are digest-only.
  - AC keys are digest plus instance name, so the instance name must match
    between a local store and a shared one, or be rewritten with
    `demultiplexing` ([CAS][bbs-cas-key], [AC][bbs-ac-key]).
- **Tiering building blocks** ([schema][bbs-tiering]):
  - `readCaching{slow, fast}`: writes go to slow, and fast is filled on read.
  - `readFallback{primary, secondary}`: reads try primary then secondary, and
    writes go only to primary.
  - `mirrored`, `sharding`, `demultiplexing` (by instance-name prefix),
    `existenceCaching`.
  - `grpc`: proxies to any REAPI server.
- **`bb_clientd` is a per-user daemon** combining a caching REAPI proxy and a
  virtual filesystem. It is meant to work with non-Buildbarn backends and to
  survive `bazel clean` and multiple checkouts ([README][bbc-readme]).
  - Remote CAS reads are cached locally, but the remote AC is not.
  - Instance names under `local/*` give a purely local AC and CAS. In the
    default config that AC has no `completenessChecking`
    ([config][bbc-config-backends]).
  - Its virtual filesystem is always mounted ([source][bbc-mount]).

**6. As a per-user service**

- **Cache-only is one binary with one Jsonnet file.** Local workers need four
  or five processes, each with its own config.
- **Listeners.** Several listeners bind to all interfaces in `bare`
  ([config][bbd-bare-frontend]). Unix-socket listeners exist
  (`listenPaths`), but buck2 needs TCP ([schema][bbs-grpc-listen]).
- **Service units.**
  - `bb_clientd` ships a systemd *user* unit and reads per-user overrides from
    `~/.config/bb_clientd/bb_clientd.jsonnet` ([README][bbc-readme-systemd]).
  - No repository ships a launchd plist.
- **Not in nixpkgs.** Every repository is a Go module with generated `.pb.go`
  files committed. A `buildGoModule` package looks feasible but is untested.

### buildbox-casd (BuildGrid project)

Read at buildbox `c8e9a010` (tag 1.4.26, 2026-09-21).

**1. Shapes and darwin**

- **One C++ binary can be the whole server.** `buildbox-casd` registers CAS,
  ByteStream, Capabilities, ActionCache, Execution and Operations
  ([source][casd-services]).
- **Local execution.** With `--buildbox-run` and no remote endpoint it runs
  actions on this host through a `LocalExecutionInstance`, with no scheduler
  process ([source][casd-local-exec]).
  - Relevant flags: `--jobs` (default 4) and `--cache-failures`
    ([README][casd-readme-exec]).
  - `--cache-failures` defaults to **true**, which would cache failing tests.
- **darwin: built by Homebrew, not by upstream.**
  - Upstream claims "support for Linux and Solaris"
    ([README][buildbox-readme]), and its CI runs on Linux only
    ([CI][buildbox-ci]).
  - Homebrew's `recc` formula builds casd and `buildbox-run-hosttools` for
    macOS ≥ Sonoma, with arm64 bottles and a launchd `service` running casd
    ([formula][brew-recc]).
  - Whether casd's local execution works on darwin is unverified.

**2. buck2**

- **No buck2 docs or example** mention buildbox.
- **casd binds a Unix socket by default.** buck2 needs
  `--bind=localhost:PORT` ([README][casd-readme-bind]).
- **Digests.** SHA256 is supported, BLAKE3 is not ([README][casd-readme]).

**3. Packaging and storage**

- **nixpkgs** ([package.nix][np-buildbox]):
  - `buildbox` 1.3.54 at turnkey's pin, 1.4.26 on master
  - `platforms = lib.platforms.linux`
  - `buildbox-run` is wrapped with bubblewrap
- **Eviction** ([README][casd-readme-bind], [docs][casd-docs-eviction]):
  - LRU with `--quota-high` / `--quota-low`, off unless set
  - ages judged by mtime
  - blob-based, ignoring references
- **Restarts.** A clean shutdown writes a `disk_usage` file, so the next start
  skips the rescan.
- **AC integrity.** `GetActionResult` answers NOT_FOUND when any output blob is
  missing ([source][casd-ac-check]).

**4. Sandboxing**

- **`buildbox-run-hosttools`** "attempts to run actions without any
  sandboxing" ([README][buildbox-hosttools]). It is the only runner that
  builds on macOS.
- **Linux runners** isolate with bubblewrap or userchroot.
- **Hardlinked staging.** An action that writes to its inputs corrupts the
  cache unless casd runs as a different user from the client
  ([README][casd-readme-hardlink]).

**5. Tiering**

- **casd proxies all three services**: CAS with `--cas-remote`, AC with
  `--ac-remote`, and execution with `--exec-remote`
  ([README][casd-readme-proxy]). `--exec-fallback-to-local` runs actions
  locally when the remote cannot.
- **AC reads:** local first, then remote, storing a remote hit locally
  ([source][casd-ac-proxy-read]).
- **AC writes:** upstream first, then local, unless `--read-only-remote`
  ([source][casd-ac-proxy-write]).
- **Keys pass through unchanged.** `--proxy-instance=LOCAL:REMOTE` maps
  instance names.

**6. As a per-user service**

- **One binary, flags only**, and Homebrew already runs it under launchd
  ([formula][brew-recc]).
- **Main drawbacks** for turnkey:
  - Linux-only in nixpkgs.
  - No upstream darwin CI.
  - Failures are cached by default.

### BuildBuddy (open-source server)

Read at v2.310.0 (commit `91e473f2`, released 2026-09-24).

- **Licence split.** `enterprise/` is under the BuildBuddy Enterprise License;
  everything else is MIT ([LICENSE][bby-license]). The enterprise licence
  allows use "for development and testing purposes" without a subscription,
  but production use needs one ([enterprise LICENSE][bby-ent-license]).
- **Remote execution is enterprise-only.**
  - The OSS server registers Execution and Scheduler only when enterprise code
    provides them ([source][bby-register]).
  - The docs: "Remote Build Execution is only configurable in the Enterprise
    version" ([docs][bby-rbe-docs]).
- **Cache-only is fully OSS.** AC, CAS, ByteStream and Capabilities
  ([source][bby-register]).
- **Executors.** They have native darwin builds and a documented macOS
  LaunchAgent setup ([docs][bby-mac-rbe]). All isolation types are enterprise
  code ([docs][bby-isolation]).
  - `sandbox` on darwin wraps actions in `sandbox-exec` with a profile that
    starts from `(allow default)`. It denies only reads of sibling action
    directories, so `/usr`, `/nix/store` and `$HOME` stay readable
    ([source][bby-sandbox]).
- **buck2.**
  - [`examples/remote_execution/buildbuddy`][b2-ex-buildbuddy] targets the
    hosted service. buck2's Linux CI builds `examples/persistent_worker`
    against BuildBuddy ([workflow][b2-ci]).
  - A local OSS server would take `grpc://127.0.0.1:1985`
    ([source][bby-port]) and `tls = false`. Anonymous users get `CACHE_WRITE`,
    so buck2's upload probe would pass ([source][bby-anon-caps]). This is
    inferred and untested.
- **Footprint** ([db][bby-db], [main][bby-main-ports]):
  - The OSS server is about 94 MB.
  - It always opens a database (default `sqlite3:///tmp/buildbuddy.db`) and a
    blob store.
  - It serves a web UI on 8080 and monitoring on 9090.
- **Cache size and eviction.**
  - `cache.max_size_bytes` defaults to 10 GB ([source][bby-cache-size]).
  - Eviction is LRU by atime, starting at 90% of the limit.
  - The ledger is rebuilt at startup by walking the directory
    ([source][bby-disk-cache]).
- **Keys.**
  - Digests are standard, including SHA256 and BLAKE3
    ([source][bby-digests]).
  - The instance name is part of the on-disk key for both AC and CAS
    ([source][bby-partition]).
- **AC integrity.** `GetActionResult` checks that referenced outputs exist
  ([source][bby-ac-check]).
- **Tiering is enterprise-only.** The Cache Proxy is enterprise; the OSS
  backends are disk and memory ([docs][bby-proxy]).
- **Running it per user:**
  - listens on `0.0.0.0` by default ([source][bby-listen])
  - uses four ports
  - sends telemetry daily unless `--disable_telemetry` ([source][bby-telemetry])
  - OSS darwin release binary is amd64 only ([release][bby-release])
  - no nixpkgs package and no service units for the OSS server

### Less suitable options

- **BuildGrid server** (Python ≥ 3.12). It checks the AC against the CAS
  ([source][bg-ac-check]) and can layer caches.
  - Execution needs PostgreSQL, and its SQL provider rejects SQLite
    ([source][bg-sql]).
  - Its in-memory AC is lost on restart ([source][bg-lru]).
  - A persistent AC means Redis or S3 ([source][bg-parser]).
  - Too heavy for a per-user service.
- **Bazel Buildfarm** (`buildfarm/buildfarm`, Java).
  - Redis is required. It holds the worker registry, queues, CAS index and AC
    ([README][bf-readme], [docs][bf-instance-types]).
  - Even cache-only needs a shard worker ([quick start][bf-quickstart]).
  - The README lists buck2 among its clients.
  - A `macos-wrapper.sh` exists, but macOS workers are not documented
    ([source][bf-macos-wrapper]).
  - Too heavy for a per-user service.
- **justbuild `just execute`** (v1.6.6).
  - It implements Execution, AC, CAS, ByteStream and Capabilities
    ([source][jb-services]).
  - `UpdateActionResult` returns UNIMPLEMENTED and it advertises
    `update_enabled = false` ([AC][jb-ac], [caps][jb-caps]), so it cannot
    accept buck2's uploads.
  - buck2's upload probe treats only PERMISSION_DENIED as "no permission"
    ([source][b2-perm-check]), so uploads would likely fail with errors.
  - buck2 would need `--compatible` (plain SHA256) ([tutorial][jb-compatible]).
  - Actions run unsandboxed with platform properties dropped
    ([source][jb-exec-props]).
  - Upstream lists Linux only ([INSTALL][jb-install]), and nixpkgs marks it
    broken on darwin ([package.nix][np-justbuild]).
- **EngFlow.**
  - The only single-node offerings are x86_64 Linux Docker images.
    - EngFlow Free has had no commit since 2022 ([repo][ef-free]).
    - MyEngFlow Mini is cache-only and needs an account ([repo][ef-mini],
      [docs][ef-setup]).
  - Both are proprietary and licence-gated ([terms][ef-free-terms]).
  - macOS workers exist only in the paid product ([options][ef-options]).
  - buck2 has an example for the hosted service
    ([`examples/remote_execution/engflow`][b2-ex-engflow]).
- **Bazel's remote worker** (`src/tools/remote`). It is a development and test
  tool built from Bazel source.
  - Sandboxing is Linux-only ([README][bz-worker-readme],
    [source][bz-worker-sandbox]).
  - buck2 users ran it locally and hit gRPC message-size limits
    ([buck2#583][b2-583], [buck2#563][b2-563]).
- **Not servers, or abandoned.**
  - quokka is a buck2 test runner, not a server ([README][quokka-readme]).
  - `thoughtpolice/bucktools`' cache server is an experiment, last touched in
    2024 ([tree][bucktools]).
  - turbo-cache became NativeLink.
  - Aspect Workflows is a managed Buildbarn, not laptop-local
    ([docs][aspect]).
  - sccache does not speak REAPI.

## What buck2 requires of any server

Every buck2 claim in this section was read at buck2 HEAD [`b8ae22e6`][b2]
(2026-09-24). Turnkey pins buck2-2026-04-15. Whether that binary behaves the
same way is the question of turnkey-w55.1.

### Client configuration

- **`[buck2_re_client]` keys in the OSS build** ([struct][b2-oss-cfg],
  [parser][b2-oss-parse]):
  - endpoints: `address` (sets all three), `engine_address`,
    `action_cache_address`, `cas_address`
  - security: `tls`, `tls_ca_certs`, `tls_client_cert`, `http_headers`
  - protocol: `capabilities`, `instance_name`
  - message sizing: `max_decoding_message_size`, `max_total_batch_size`,
    `max_concurrent_uploads_per_action`, `find_missing_blobs_batch_size`,
    `cas_ttl_secs`
  - connections: keepalive, concurrency and pool keys
- **No local-daemon keys.** The `cas_shared_cache*` ("shared casd") keys exist
  only in the internal fbcode configuration ([source][b2-fb-cfg]). OSS buck2
  has no built-in local CAS tier.
- **`tls` defaults to `true`.** A plaintext localhost server needs
  `tls = false` ([source][b2-tls-default]).
- **`engine_address` is mandatory, even for cache-only.** The client fails with
  "No engine address" without it, and sends `GetCapabilities` there unless
  `capabilities = false` ([source][b2-caps]).
- **TCP only.**
  - URIs may use only the `grpc`, `dns`, `ipv4` or `ipv6` scheme, or none
    ([source][b2-scheme]).
  - The channel uses a TCP `HttpConnector` ([source][b2-connector]).
  - A per-user service must listen on loopback TCP, which means choosing a
    port per user on shared Linux hosts.
- **Hash functions: use SHA256.**
  - OSS buck2 defaults to SHA256 ([source][b2-digest-default]).
  - The OSS client never fills in REAPI's `digest_function` field. It does not
    appear in `remote_execution/oss/re_grpc/src/`.
  - The spec says a server then infers the function from the hash length
    ([spec][reapi-digest-fn]).
  - BLAKE3 hashes are the same length as SHA256, so BLAKE3 works only against
    a server that treats unset requests as BLAKE3.
- **Platform properties go only on `Command.platform`**
  ([source][b2-command-platform]). REAPI v2.2 deprecated that field and says
  servers SHOULD prefer `Action.platform`
  ([Action][reapi-action-platform], [Command][reapi-command-platform]). This
  is the root of the Buildbarn break above.
- **Batch size.** With no server limit, the client batches up to 4,000,000
  bytes. Its decode limit defaults to twice the batch size
  ([batch][b2-batch], [decode][b2-decode]).

### How test results reach the action cache

- **Where cached test results come from.**
  - In the `Testing` stage buck2 consults the AC only when the test rule
    reports `supports_test_execution_caching`
    ([source][b2-test-cache-lookup]).
  - It never attaches an uploader: "We never upload local test executions"
    ([source][b2-test-no-upload]).
  - A cached test result can therefore only come from an AC entry written by
    the server when it executed the test. REAPI lets servers cache results
    unless `do_not_cache` is set ([spec][reapi-action]), and buck2 always
    sends `false` ([source][b2-do-not-cache]).
- **The flag defaults to false** ([source][b2-test-flag-default]). Upstream
  exposes it only on some rules:
  - declared by the cxx, Python, shell, Java/Kotlin and Android rules
    (e.g. [python][b2-prelude-python], [sh][b2-prelude-sh])
  - absent from `go_test` ([source][b2-prelude-go]) and `rust_test`
    ([source][b2-prelude-rust])
- **Which runs get cached depends on the server.**
  - NativeLink workers cache only exit code 0 ([source][nl-success-only]).
  - casd caches failures by default ([README][casd-readme-exec]).
- **Test listings are different.** When cacheable they are looked up and
  uploaded even when run locally ([source][b2-test-listing-upload]).
- **Local test runs see a small environment:** `PATH`, `USER`, `LOGNAME`,
  `HOME`, `TMPDIR` and `XDG_RUNTIME_DIR` ([source][b2-test-env],
  [allowlist][b2-env-allowlist]).

### The cache-only shape

- **The executor config.** `local_enabled = True`, `remote_enabled = False`,
  `remote_cache_enabled = True` and `allow_cache_uploads = True` give a
  local-only executor that still reads and writes the RE cache
  ([source][b2-cec-localonly], [upload flag][b2-cec-upload]).
  - In OSS, `remote_cache_enabled` defaults to the value of `remote_enabled`,
    so it has to be set explicitly ([source][b2-cec-cache-default]).
- **Upload permission probe.** Before uploading, buck2 writes an empty
  ActionResult. A `PERMISSION_DENIED` answer turns uploads off; any other error
  is returned ([source][b2-perm-check]).
- **Locally run build actions are not sandboxed.**
  - They run in the project root ([source][b2-local-cwd]).
  - They inherit the daemon's whole environment except five variables
    (`PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `LD_LIBRARY_PATH`,
    `LD_PRELOAD`) ([request][b2-local-env], [exclusions][b2-env-exclusions]).
  - Anything an action reads outside its declared inputs and env is therefore
    missing from the key of the result buck2 uploads.
  - Workers (shape 1) start from a cleared environment and stage only declared
    inputs. On darwin, though, none of the OSS candidates hides the rest of
    the host filesystem.

### The action cache must not point at missing blobs

- **REAPI expectation.** Implementations "SHOULD ensure that any blobs
  referenced from the ContentAddressableStorage are available at the time of
  returning the ActionResult" ([spec][reapi-ac-complete]).
- **buck2 relies on it.** In OSS builds a dangling reference is a hard build
  error that survives daemon restarts. A maintainer asked for the fix on the
  server side ([buck2#862][b2-862]).
- **Any local cache with eviction must enforce it:**
  - bazel-remote, BuildBuddy and buildbox-casd do by default.
  - NativeLink and Buildbarn do only when configured.

### What goes into the cache key

- **The action digest is computed by buck2**, as the hash of an `Action`
  holding ([source][b2-action-digest]):
  - the input-root digest
  - the `Command` digest: arguments, environment, platform properties,
    working directory and output paths
  - timeout and `do_not_cache`
- **None of it comes from the server.** Swapping servers leaves keys unchanged
  as long as the client-side inputs stay the same:
  - digest function
  - `remote_execution_properties`
  - `remote_output_paths` mode ([source][b2-output-paths-cmd])
  - timeouts
- **`salt` could separate namespaces** ([spec][reapi-salt]), but buck2 never
  sets it.
- **The instance name is not part of the digest**, but it does namespace the
  AC:
  - REAPI lets servers use it to select a partition ([spec][reapi-instance]).
  - Buildbarn and BuildBuddy key their AC by it.
  - NativeLink uses it to pick a store.
  - bazel-remote ignores it by default.
- **External symlinks.** Symlinks that leave the project, such as turnkey's
  `/nix/store` toolchain links, are sent as REAPI `SymlinkNode`s with their
  absolute target ([source][b2-ext-symlink]). Their contents are never
  uploaded.
  - A localhost worker resolves them against the host's `/nix/store`.
  - A shared worker needs the same store paths.
  - The store path in the key identifies the toolchain.

## Swapping or federating with a shared backend later

- **Replacing the server leaves keys unchanged** as long as buck2's digest
  function, platform properties, output-path mode and instance name stay the
  same. Digests are computed client-side
  ([source][b2-action-digest]).
- **Shape 1 caveat.** If a shared backend's workers need different platform
  properties (a Linux `container-image`, say), those tests get different keys.
  That is correct: they run on a different platform, and reusing results
  across platforms is not a goal.
- **A local tier in front of a shared one exists in**:
  - **bazel-remote:** `--grpc_proxy.url`, read-through with asynchronous
    write-through.
  - **NativeLink:** `fast_slow` over a `grpc` store, with
    `slow_direction: read_only` for a "diode".
  - **Buildbarn:** `readCaching` / `readFallback` over a `grpc` backend, and
    `bb_clientd`.
  - **buildbox-casd:** `--cas-remote`, `--ac-remote`, `--exec-remote`, with
    `--read-only-remote`.
  - **BuildBuddy:** enterprise only.
- **The trust model is decided in the tier's write direction.** A read-only
  upstream lets a laptop read CI's results without publishing its own. That
  maps onto the open question of who may write to the shared cache.

## Facts most decisive for the mechanism choice (turnkey-w55.7)

1. **Only NativeLink** (one process, one config) **and Buildbarn**
   (4–5 processes) offer freely usable local workers that run natively on
   darwin today (shape 1).
   - Neither is in nixpkgs.
   - NativeLink's darwin build gets only a start-up smoke test in CI, and its
     licence is FSL, not OSI.
   - Buildbarn needs buck2#1366 or a worker registered with an empty platform.
   - buildbox-casd may also work on darwin via Homebrew, but that is
     unverified.
   - BuildBuddy's executor is enterprise-licensed.
2. **Cache-only (shape 2) has a clear fit: bazel-remote.**
   - nixpkgs on darwin and Linux, prebuilt.
   - One Go binary with an LRU `--max_size`.
   - Checks the AC against the CAS by default.
   - gRPC proxy to a shared backend later.
3. **No OSS candidate sandboxes a darwin worker.**
   - NativeLink and Buildbarn do give each action a clean environment and a
     directory containing only its declared inputs.
   - Absolute paths (`/nix/store`, `$HOME`) stay readable.
4. **buck2 treats a dangling AC→CAS reference as a hard error.** Whatever runs
   locally must enforce completeness. That is the default only in
   bazel-remote, BuildBuddy and casd; NativeLink and Buildbarn need it
   configured.
5. **Cache keys are computed entirely by buck2.** A local store can be
   replaced by, or put in front of, a shared REAPI backend without changing
   keys, provided the digest function (SHA256), platform properties,
   output-path mode and instance-name handling match.
6. **buck2 reaches the server only over TCP, with `tls` on by default, and
   needs an `engine_address` that answers `GetCapabilities`.** A per-user
   service needs a loopback port, and on shared Linux hosts one per user.
7. **Test results reach the AC only through a worker, and only for rules that
   set `supports_test_execution_caching`.** Upstream `rust_test` and `go_test`
   don't set it.
8. **Failing tests.** NativeLink workers cache exit code 0 only; casd caches
   failures by default.

## Unverified or open

- Memory use and startup time of NativeLink, Buildbarn and bazel-remote. No
  project publishes figures, and nothing was run for this research.
- Whether buildbox-casd's local execution works on darwin (Homebrew builds the
  runner but only runs casd as a cache), and how its scheduler handles
  platform properties.
- Whether a non-root user can make Buildbarn's NFSv4 mount on macOS. The
  native hardlinking build directory avoids the question.
- A real buck2 run against each local server. Nothing here was executed. The
  compatibility claims come from source reading plus the buck2 and NativeLink
  CI configs.
- Buildbarn's AC policy for non-zero exit codes was not checked.

## Sources

Pinned revisions:

- buck2 [`b8ae22e6`][b2]
- remote-apis `adbf4a27`
- nixpkgs `f8573b9c` (turnkey's `flake.lock`) and master `82cb766c`
  (2026-09-25)
- NativeLink `612fff0b` (v1.7.1)
- bazel-remote `3cb3084b` (v2.6.2)
- Buildbarn: bb-storage `b0032b1d`, bb-remote-execution `c7bbd666`,
  bb-deployments `a3548560`, bb-clientd `85d2ce6e`, bb-adrs `00164e0c`
- buildbox `c8e9a010`
- BuildGrid `d62da23c`
- Buildfarm `ebe46ee1`
- BuildBuddy `91e473f2` (v2.310.0)
- justbuild v1.6.6
- Bazel `151450cd`

<!-- buck2 -->
[b2]: https://github.com/facebook/buck2/tree/b8ae22e6732343863e079049958e362b157f0cc8
[b2-oss-cfg]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_re_configuration/src/lib.rs#L421-L486
[b2-oss-parse]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_re_configuration/src/lib.rs#L513-L625
[b2-fb-cfg]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_re_configuration/src/lib.rs#L93-L160
[b2-tls-default]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_re_configuration/src/lib.rs#L541-L546
[b2-caps]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/client.rs#L252-L282
[b2-batch]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/client.rs#L376-L393
[b2-decode]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/client.rs#L284-L292
[b2-bytestream-zstd]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/client.rs#L294-L300
[b2-scheme]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/pool.rs#L138-L157
[b2-connector]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/remote_execution/oss/re_grpc/src/pool.rs#L205-L217
[b2-digest-default]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_server/src/daemon/state.rs#L334-L343
[b2-command-platform]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/command_executor.rs#L328-L341
[b2-action-digest]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/command_executor.rs#L391-L406
[b2-do-not-cache]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/command_executor.rs#L236-L246
[b2-output-paths-cmd]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/command_executor.rs#L343-L389
[b2-output-paths-default]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_core/src/execution_types/executor_config.rs#L459-L467
[b2-ext-symlink]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/directory.rs#L202-L208
[b2-test-cache-lookup]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_test/src/orchestrator.rs#L1325-L1354
[b2-test-no-upload]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_test/src/orchestrator.rs#L1527-L1540
[b2-test-listing-upload]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_test/src/orchestrator.rs#L1258-L1322
[b2-test-env]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_test/src/orchestrator.rs#L1809-L1811
[b2-env-allowlist]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/environment_inheritance.rs#L19-L28
[b2-env-exclusions]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute/src/execute/environment_inheritance.rs#L105-L119
[b2-local-env]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_action_impl/src/actions/impls/run.rs#L1305-L1312
[b2-local-cwd]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute_impl/src/executors/local.rs#L197
[b2-test-flag-default]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L204-L210
[b2-prelude-python]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/prelude/decls/python_rules.bzl#L147
[b2-prelude-sh]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/prelude/decls/shell_rules.bzl#L199
[b2-prelude-go]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/prelude/go/go_test.bzl#L179-L189
[b2-prelude-rust]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/prelude/rust/rust_binary.bzl#L849-L859
[b2-cec-upload]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L312-L318
[b2-cec-cache-default]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L320-L330
[b2-cec-localonly]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L364-L384
[b2-perm-check]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/app/buck2_execute_impl/src/executors/action_cache_upload_permission_checker.rs#L53-L82
[b2-ex-nativelink]: https://github.com/facebook/buck2/tree/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/nativelink
[b2-ex-nativelink-readme]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/nativelink/README.md#L19-L55
[b2-ex-buildbarn]: https://github.com/facebook/buck2/tree/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/buildbarn
[b2-ex-buildbarn-readme]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/buildbarn/README.md#L13-L41
[b2-ex-buildbuddy]: https://github.com/facebook/buck2/tree/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/buildbuddy
[b2-ex-engflow]: https://github.com/facebook/buck2/tree/b8ae22e6732343863e079049958e362b157f0cc8/examples/remote_execution/engflow
[b2-ci]: https://github.com/facebook/buck2/blob/b8ae22e6732343863e079049958e362b157f0cc8/.github/workflows/build-and-examples.yml#L45-L79
[b2-862]: https://github.com/facebook/buck2/issues/862
[b2-1477]: https://github.com/facebook/buck2/issues/1477
[b2-1366]: https://github.com/facebook/buck2/pull/1366
[b2-1475]: https://github.com/facebook/buck2/issues/1475
[b2-583]: https://github.com/facebook/buck2/issues/583
[b2-563]: https://github.com/facebook/buck2/issues/563

<!-- REAPI -->
[reapi-ac-complete]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L157-L165
[reapi-action]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L653-L674
[reapi-salt]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L723-L729
[reapi-action-platform]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L731-L739
[reapi-command-platform]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L878-L881
[reapi-instance]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1583-L1590
[reapi-digest-fn]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1622-L1629

<!-- nixpkgs -->
[np-bazel-remote]: https://github.com/NixOS/nixpkgs/blob/f8573b9c935cfaa162dd62cc9e75ae2db86f85df/pkgs/by-name/ba/bazel-remote/package.nix#L7-L38
[np-buildbox]: https://github.com/NixOS/nixpkgs/blob/f8573b9c935cfaa162dd62cc9e75ae2db86f85df/pkgs/by-name/bu/buildbox/package.nix#L24-L70
[np-justbuild]: https://github.com/NixOS/nixpkgs/blob/f8573b9c935cfaa162dd62cc9e75ae2db86f85df/pkgs/by-name/ju/justbuild/package.nix#L180

<!-- NativeLink -->
[nl-release]: https://github.com/TraceMachina/nativelink/releases/tag/v1.7.1
[nl-release-ci]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/.github/workflows/release.yaml#L140-L164
[nl-nix-ci]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/.github/workflows/nix.yaml#L132-L151
[nl-cargo-ci]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/.github/workflows/native-cargo.yaml#L33-L34
[nl-top-config]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/cas_server.rs#L1215-L1229
[nl-caps-no-re]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/cas_server.rs#L258-L262
[nl-basic-cas]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/examples/basic_cas.json5#L1-L36
[nl-basic-cas-props]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/examples/basic_cas.json5#L59-L61
[nl-basic-cas-listen]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/examples/basic_cas.json5#L136-L139
[nl-docs-local-testing]: https://docs.nativelink.com/remote-execution/local-testing
[nl-docs-namespaces]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/remote-execution/first-remote-action.mdx#L234-L238
[nl-buck2-buckconfig]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/integration_tests/buck2/.buckconfig#L4-L9
[nl-buck2-cas]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/integration_tests/buck2/buck2_cas.json5#L1-L20
[nl-buck2-platform]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/integration_tests/buck2/platforms/defs.bzl#L21-L28
[nl-ac-instance]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-service/src/ac_server.rs#L86-L90
[nl-props]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/schedulers.rs#L43-L65
[nl-completeness]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L284-L309
[nl-docs-compose]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/how-to/stores/compose-stores.mdx#L150-L168
[nl-flake-systems]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/flake.nix#L34-L39
[nl-flake-packages]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/flake.nix#L461-L528
[nl-docs-quickstart]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/getting-started/quickstart.mdx#L52-L62
[nl-stores]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L52-L646
[nl-eviction]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L1183-L1213
[nl-memory-unbounded]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L1011-L1016
[nl-fs-persistent]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L495-L511
[nl-fs-startup]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-store/src/filesystem_store.rs#L1050-L1075
[nl-worker-hardlink]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-worker/src/running_actions_manager.rs#L446-L462
[nl-worker-env]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-worker/src/running_actions_manager.rs#L1833-L1915
[nl-docs-worker-exec]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/explanations/worker-execution.mdx#L128
[nl-docs-worker-exec-ns]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/explanations/worker-execution.mdx#L102-L109
[nl-ns]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-worker/src/namespace_utils.rs#L401-L415
[nl-darwin-ns]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-worker/src/local_worker.rs#L698-L714
[nl-success-only]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/cas_server.rs#L770-L786
[nl-ac-key]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-service/src/ac_server.rs#L93-L105
[nl-hashers]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-util/src/digest_hasher.rs#L53-L76
[nl-fast-slow]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L956-L994
[nl-grpc-store]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-store/src/grpc_store.rs#L1098-L1109
[nl-docs-migrate]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/how-to/migrate-an-existing-cache.mdx#L30-L104
[nl-grpc-ac-caveat]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/stores.rs#L556-L564
[nl-docs-cli]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/reference/cli-and-env.mdx#L20-L35
[nl-docs-bare-metal]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/operate/deploy-bare-metal.mdx#L17
[nl-listener]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/cas_server.rs#L674-L686
[nl-read-only]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-config/src/cas_server.rs#L112-L125
[nl-ac-read-only]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/nativelink-service/src/ac_server.rs#L131-L136
[nl-license]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/LICENSE#L1-L4
[nl-docs-oss-enterprise]: https://github.com/TraceMachina/nativelink/blob/612fff0b0eadbcc6dd783696327c7ec0a092c15b/web/apps/docs/content/docs/reference/oss-and-enterprise.mdx#L37

<!-- bazel-remote -->
[br-releases]: https://github.com/buchgr/bazel-remote/releases
[br-services]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/server/grpc.go#L92-L98
[br-caps]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/server/grpc.go#L109-L138
[br-flags-grpc]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/utils/flags/flags.go#L90-L101
[br-flags-idle]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/utils/flags/flags.go#L177
[br-readme-mangling]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L45-L50
[br-ac-mangling]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/server/grpc_ac.go#L60-L62
[br-readme-size]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L152-L156
[br-readme-lru]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L10-L13
[br-readme-hard-limit]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L469-L479
[br-readme-storage-mode]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L158-L159
[br-load]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/disk/load.go#L568-L581
[br-main-order]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/main.go#L162-L210
[br-lru]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/disk/lru.go#L17-L38
[br-systemd]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/examples/bazel-remote.service
[br-ac-validate]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/disk/disk.go#L816-L916
[br-ac-update]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/server/grpc_ac.go#L223-L272
[br-readme-auth]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L205-L226
[br-readme-proxy]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L552-L611
[br-disk-proxy]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/disk/disk.go#L539-L558
[br-grpcproxy-queue]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/grpcproxy/grpcproxy.go#L210-L230
[br-grpcproxy-digest]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/grpcproxy/grpcproxy.go#L135-L143
[br-grpcproxy-caps]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/grpcproxy/grpcproxy.go#L59-L73
[br-readme-config]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/README.md#L130-L134
[br-919]: https://github.com/buchgr/bazel-remote/issues/919

<!-- Buildbarn -->
[bbs-main]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/cmd/bb_storage/main.go#L73-L107
[bbs-release]: https://github.com/buildbarn/bb-storage/releases/tag/20260924T123715Z-b0032b1
[bbs-digest-functions]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/digest/bare_function.go#L16-L28
[bbs-blockdevice]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blockdevice/new_block_device_from_file_unix.go#L19-L49
[bbs-local-schema]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/blobstore/blobstore.proto#L451-L526
[bbs-persistent]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/blobstore/blobstore.proto#L563-L590
[bbs-completeness]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/blobstore/blobstore.proto#L659-L673
[bbs-cas-key]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/configuration/cas_blob_access_creator.go#L60
[bbs-ac-key]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/configuration/ac_blob_access_creator.go#L119
[bbs-tiering]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/blobstore/blobstore.proto#L29-L156
[bbs-grpc-listen]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/grpc/grpc.proto#L150-L165
[bbr-release]: https://github.com/buildbarn/bb-remote-execution/releases/tag/20260923T083757Z-c7bbd66
[bbr-nfsv4]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/filesystem/virtual/virtual.proto#L18-L38
[bbr-nfsv4-darwin]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/filesystem/virtual/configuration/nfsv4_mount_darwin.go#L45-L196
[bbr-fuse-disabled]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/filesystem/virtual/configuration/fuse_mount_disabled.go#L1-L16
[bbr-worker-builddir]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_worker/bb_worker.proto#L181-L235
[bbr-worker-env]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_worker/bb_worker.proto#L368-L375
[bbr-action-key]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/scheduler/platform/action_key_extractor.go#L10-L22
[bbr-sched-proto]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/scheduler/scheduler.proto#L108-L130
[bbr-pr-202]: https://github.com/buildbarn/bb-remote-execution/pull/202
[bbr-output-paths]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/builder/output_hierarchy.go#L410-L433
[bbr-runner-env]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/runner/local_runner.go#L148-L170
[bbr-runner-chroot]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_runner/bb_runner.proto#L36-L39
[bbr-runner-runas]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_runner/bb_runner.proto#L61-L64
[bbr-runner-xcode]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/proto/configuration/bb_runner/bb_runner.proto#L95-L137
[bbd-bare-readme]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/README.md#L1-L19
[bbd-bare-worker]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/worker.jsonnet#L65-L79
[bbd-bare-runner]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/runner.jsonnet#L8
[bbd-bare-scheduler]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/scheduler.jsonnet#L30
[bbd-bare-storage]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/storage.jsonnet#L28
[bbd-bare-common]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/common.libsonnet#L21
[bbd-bare-frontend]: https://github.com/buildbarn/bb-deployments/blob/a35485609467dd70cd44c78b4735e5835061df5a/bare/config/frontend.jsonnet#L5
[bbc-readme]: https://github.com/buildbarn/bb-clientd/blob/85d2ce6e43887e1642cd1f9048e7dbe611380785/README.md#L3-L14
[bbc-readme-systemd]: https://github.com/buildbarn/bb-clientd/blob/85d2ce6e43887e1642cd1f9048e7dbe611380785/README.md#L60-L89
[bbc-config-size]: https://github.com/buildbarn/bb-clientd/blob/85d2ce6e43887e1642cd1f9048e7dbe611380785/configs/bb_clientd.jsonnet#L13
[bbc-config-backends]: https://github.com/buildbarn/bb-clientd/blob/85d2ce6e43887e1642cd1f9048e7dbe611380785/configs/bb_clientd.jsonnet#L82-L147
[bbc-mount]: https://github.com/buildbarn/bb-clientd/blob/85d2ce6e43887e1642cd1f9048e7dbe611380785/cmd/bb_clientd/main.go#L105-L114
[bba-0009]: https://github.com/buildbarn/bb-adrs/blob/00164e0caac384cc3c76e875773a1053fb1c4ef6/0009-nfsv4.md

<!-- buildbox / BuildGrid / Buildfarm -->
[casd-services]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/buildboxcasd_server.cpp#L529-541
[casd-local-exec]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/buildboxcasd_server.cpp#L477-489
[casd-readme]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/README.rst#L138-139
[casd-readme-bind]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/README.rst#L187-195
[casd-readme-exec]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/README.rst#L196-201
[casd-readme-proxy]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/README.rst#L120-185
[casd-readme-hardlink]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/README.rst#L237-247
[casd-docs-eviction]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/docs/casd.md#L128-141
[casd-ac-check]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/buildboxcasd_localacinstance.cpp#L62-83
[casd-ac-proxy-read]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/buildboxcasd_localacproxyinstance.cpp#L84-121
[casd-ac-proxy-write]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/casd/buildboxcasd_localacproxyinstance.cpp#L173-185
[buildbox-readme]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/README.rst#L6
[buildbox-ci]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/.gitlab-ci.yml#L24-26
[buildbox-hosttools]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/c8e9a010f07edb46542b637f51fce3ac34427d3c/run-hosttools/README.rst#L4
[brew-recc]: https://github.com/Homebrew/homebrew-core/blob/a935f1ff4441ed939d65feb530b208b2384bd891/Formula/r/recc.rb#L9-L90
[bg-ac-check]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/actioncache/caches/action_cache_abc.py#L66-157
[bg-sql]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/sql/provider.py#L237-249
[bg-lru]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/actioncache/caches/lru_cache.py#L29-33
[bg-parser]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/app/settings/parser.py#L1835
[bf-readme]: https://github.com/buildfarm/buildfarm/blob/ebe46ee176d6ce22ab298db62e8114488c7bb69d/README.md#L10-L29
[bf-instance-types]: https://github.com/buildfarm/buildfarm/blob/ebe46ee176d6ce22ab298db62e8114488c7bb69d/_site/docs/architecture/instance_types.md#L18-L20
[bf-quickstart]: https://github.com/buildfarm/buildfarm/blob/ebe46ee176d6ce22ab298db62e8114488c7bb69d/_site/docs/quick_start.md#L13-L73
[bf-macos-wrapper]: https://github.com/buildfarm/buildfarm/blob/ebe46ee176d6ce22ab298db62e8114488c7bb69d/macos-wrapper.sh#L16-L19

<!-- BuildBuddy -->
[bby-release]: https://github.com/buildbuddy-io/buildbuddy/releases/tag/v2.310.0
[bby-license]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/LICENSE
[bby-ent-license]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/enterprise/LICENSE#L6-L20
[bby-register]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/libmain/libmain.go#L288-L318
[bby-rbe-docs]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/docs/config-rbe.md#L7
[bby-mac-rbe]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/docs/enterprise-mac-rbe.md#L69-L188
[bby-isolation]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/docs/rbe-platforms.md#L255-L257
[bby-sandbox]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/enterprise/server/remote_execution/containers/sandbox/sandbox.go#L194-L309
[bby-port]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/util/grpc_server/grpc_server.go#L45-L49
[bby-anon-caps]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/util/capabilities/capabilities.go#L17-L66
[bby-db]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/util/db/db.go#L70
[bby-main-ports]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/libmain/libmain.go#L79-L83
[bby-cache-size]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/cache/config/config.go#L9
[bby-disk-cache]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/backends/disk_cache/disk_cache.go#L493-L537
[bby-digests]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/remote_cache/digest/digest.go#L49-L55
[bby-partition]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/backends/disk_cache/disk_cache.go#L310-L329
[bby-ac-check]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/remote_cache/action_cache_server/action_cache_server.go#L153-L175
[bby-proxy]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/docs/enterprise-proxy.md#L7
[bby-listen]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/util/grpc_server/grpc_server.go#L141
[bby-telemetry]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/telemetry/telemetry_client.go#L27-L30

<!-- justbuild, EngFlow, Bazel, others -->
[jb-services]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/src/buildtool/execution_api/execution_service/server_implementation.cpp#L83-L97
[jb-ac]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/src/buildtool/execution_api/execution_service/ac_server.cpp#L64-L71
[jb-caps]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/src/buildtool/execution_api/execution_service/capabilities_server.cpp#L33
[jb-compatible]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/doc/tutorial/just-execute.org#L89-L97
[jb-exec-props]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/src/buildtool/execution_api/execution_service/execution_server.cpp#L109-L126
[jb-install]: https://github.com/just-buildsystem/justbuild/blob/v1.6.6/INSTALL.md#L28-L30
[ef-free]: https://github.com/EngFlow/free
[ef-free-terms]: https://github.com/EngFlow/free#terms
[ef-mini]: https://github.com/EngFlow/myengflow_mini
[ef-setup]: https://docs.engflow.com/getting-started/set-up-engflow.html
[ef-options]: https://docs.engflow.com/re/config/options.html
[bz-worker-readme]: https://github.com/bazelbuild/bazel/blob/151450cd52144e37a07ba2193763d8812ddbe12b/src/tools/remote/README.md#L1-L48
[bz-worker-sandbox]: https://github.com/bazelbuild/bazel/blob/151450cd52144e37a07ba2193763d8812ddbe12b/src/tools/remote/src/main/java/com/google/devtools/build/remote/worker/RemoteWorker.java#L441-L479
[quokka-readme]: https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/README.md
[bucktools]: https://github.com/thoughtpolice/bucktools/tree/1d3fc309950704a26747298f2366c0af29d68e5f/tools/cache-server
[aspect]: https://docs.aspect.build/workflows/features/remote-cache/
