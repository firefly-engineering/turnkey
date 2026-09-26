# REAPI implementations and components for a request-driven executor

Research for **turnkey-wit.2**, part of the wayfinder map turnkey-wit (build
service). All sources read on 2026-09-26, at the revisions listed in
[Revisions read](#revisions-read). Claims marked *(inferred)* are conclusions
drawn from the cited code, not statements the sources make. Claims marked
*(unverified)* were not confirmed against a primary source.

The question is which existing REAPI servers or components fit an executor
where each action runs inside the `Execute` request that asked for it. In that
design:

- Cloud Run's autoscaler is the scheduler. There is no worker pool pulling
  from a queue.
- The content store (CAS) and the action cache (AC) live on GCS.
- Only the executor writes action results.

The alternative is to write our own thin REAPI server.

This note builds on [local-re-api-servers.md](local-re-api-servers.md), which
covers server behaviour on a laptop and buck2's client configuration, and on
[remote-execution-and-caching.md §4d–§4f](remote-execution-and-caching.md#4d-roll-our-own).
It does not repeat them.

## TL;DR

- **No off-the-shelf server fits.**
  - Only **justbuild `just execute`** and **buildbox-casd `--buildbox-run`**
    run the action inside `Execute`. Both are C++, store data on local disk
    only, and are not libraries.
  - Every server with GCS storage or per-client authorization puts a
    scheduler and a queue between `Execute` and the worker: NativeLink,
    BuildBuddy enterprise, BuildGrid.
- **buck2's client suits a request-driven server well.**
  - It keeps one `Execute` stream open for the whole action and reads the
    result from that stream.
  - It never calls `WaitExecution`.
  - It runs its own `GetActionResult` before `Execute`.
  - It reads only two Capabilities fields.
  - The server needs no operation store and no resume. A dropped stream is an
    action failure, which also means scale-in must drain streams.
- **Recommendation: write our own thin `Execute` handler in Go, built on
  Buildbarn's libraries.**
  - **bb-storage** supplies the CAS, AC, ByteStream and Capabilities servers.
    They include upload hash checking, per-store `put_authorizer` (which keeps
    clients from writing the AC) and AC→CAS completeness checking.
  - **bb-remote-execution**'s `LocalBuildExecutor` stages the input root, runs
    the command and uploads outputs, without the scheduler.
  - We would write:
    - a GCS `BlobAccess`, since Buildbarn removed its own
    - the `Execute` handler
    - the Nix store-path mounting (turnkey-wit.3)
- **Two facts constrain the design:**
  - Buildbarn **removed** its GCS/S3 storage backend, citing time to first
    byte, `FindMissing` cost and consistency. A per-instance cache in front of
    GCS is therefore needed, not optional *(inferred)*.
  - Cloud Run caps a request at **60 minutes**, and buck2 cannot resume an
    `Execute`. No action can run longer than that.

## 1. Comparison

| Project (revision) | Execute runs the action inside the request? | CAS/AC on GCS | Only the executor writes the AC | CAS upload hash checked | Licence, language, usable as a library |
|---|---|---|---|---|---|
| **justbuild `just execute`** (`68cd8d41`) | **Yes.** Runs inline, writes the AC itself ([execution_server.cpp L268-L318][jb-exec]) | No, local disk | **Yes**: `UpdateActionResult` returns `UNIMPLEMENTED` ([ac_server.cpp L64-L72][jb-ac]) | Yes ([cas_utils.cpp L70-L74][jb-cas]) | Apache-2.0, C++, not a library. Linux only; buck2 needs `--compatible` mode |
| **buildbox-casd `--buildbox-run`** (`efd1bfd1`) | Almost: an in-process `LocalExecutionScheduler` with `--jobs` slots runs a runner subprocess per action while the stream stays open ([header][casd-sched]) | No: local disk plus a gRPC remote | *(unverified)* | Yes ([localcasinstance.cpp L1624][casd-verify]) | Apache-2.0, C++, not a library |
| **Buildbarn** (bb-storage `b0032b1d`, bb-remote-execution `c7bbd666`) | Not as shipped (bb_scheduler plus workers that pull work). `pkg/builder` runs an action without a scheduler (§4) | **No**: the `cloud` backend was removed ([blobstore.proto L224-L245][bb-cloud-removed]) | **Yes**: per-store `get_authorizer`/`put_authorizer`, with JWT/JWKS, mTLS, JMESPath or remote checks ([bb_storage.proto L115-L147][bb-authz], [jwt.proto L18-L29][bb-jwt]) | Yes: both upload paths validate `UserProvided` buffers ([CAS L137][bb-cas-verify], [ByteStream L183][bb-bs-verify]) | Apache-2.0, Go, **importable packages**. No semver tags, so no API stability promise |
| **NativeLink** (v1.7.2 `b9d01f2c`) | No. Even an in-process worker registers with `SimpleScheduler` over gRPC and pulls from its queue ([schedulers.rs L30-L36][nl-sched], [cas_server.rs L958-L967][nl-worker-ep]) | **Yes**: `experimental_cloud_object_store`, `provider: gcs` ([stores.rs L1220-L1300][nl-gcs]) | Per endpoint (AC `read_only`), not per client ([cas_server.rs L124][nl-ro]) | Only when a `verify` store is configured; `verify_hash` is off by default ([stores.rs L1115][nl-verify]) | FSL-1.1-Apache-2.0 with a BUSL metrics module; Rust; crates not published |
| **BuildBuddy OSS** (v2.310.0) | No. Remote execution is enterprise only | Enterprise only (`enterprise/server/backends/gcs_cache`, [tree][bbd-tree]) | `CACHE_WRITE` capability ([action_cache_server.go L375][bbd-acw]); API keys are enterprise | Yes ([byte_stream_server.go L759][bbd-bs]) | MIT (OSS parts) plus enterprise licence; Go, built with Bazel |
| **BuildGrid** (0.8.11 `d62da23c`) | No: SQL scheduler plus bots (buildbox-worker) | No: disk, Redis, S3, SQL ([storage][bg-storage]). GCS through its S3 API *(unverified)* | `allow_updates` per AC instance ([parser.py L1767][bg-allow]) | Yes ([instance.py L242][bg-verify]) | Apache-2.0, Python |
| **bazel-remote** (v2.6.2 `3cb3084b`) | No execution | Only as a proxy behind a mandatory local disk cache | **No**: an authenticated writer writes both AC and CAS ([main.go L170-L260][br-auth]) | Yes ([disk.go L378-L381][br-verify]) | Apache-2.0, Go |
| **Bazel Buildfarm** (`344f3a82`) | No: Redis plus workers | No GCS code found | Not checked | Not checked | Apache-2.0, Java |
| **please-servers** (Thought Machine, `2bcbbfe`) | No. Mettle passes work over Pub/Sub. Its single-process `dual` mode is "for local testing only" ([main.go L104, L165-L185][ps-dual]) | **Yes**: Elan is a CAS/AC/ByteStream server on `gocloud.dev/blob` | No: `UpdateActionResult` has no authorization ([rpc.go L290-L303][ps-ac]) | Yes ([rpc.go L760-L775][ps-verify]) | Apache-2.0, Go. The README says it is not supported and is "for illustrative purposes only" |

What the table says:

- The request-driven shape exists in only two places, both C++ and
  local-disk (justbuild, buildbox-casd). justbuild shows how small the
  `Execute` core is: send an initial `Operation`, run the action, write the
  AC entry, send the final `ExecuteResponse`.
- No project combines GCS storage, per-client AC write control and in-request
  execution.
- Buildbarn has two of the three, and its code is layered so that the third
  can be added from outside (§4).

## 2. Prior art: serverless remote execution

- **No public REAPI executor runs actions inside Lambda, Cloud Run, Cloud
  Functions or Fargate requests.** This is a search result, not a proof
  *(unverified negative)*.
  - The nearest project, hchauvin/bazel-cloud-infra, ran Buildfarm on ECS and
    used Lambda only for cluster management. It was archived on 2020-03-05
    ([repo][hc-infra]).
- **gg** (Stanford, USENIX ATC 2019; [paper][gg-paper], [repo][gg-repo], last
  commit 2022-05-25).
  - It is not REAPI: it uses content-addressed "thunks", and gcc and ld need
    "model substitution".
  - It ran on AWS Lambda, Google Cloud Functions and OpenWhisk, with S3 or GCS
    storage.
  - Results:
    - A cold Inkscape build on Lambda was about 5× faster than icecc on a warm
      384-core cluster.
    - Measured overhead to invoke a dependent thunk was 142 ms ± 135 ms.
    - The authors conclude it "works well when thunks last about 1–20 seconds
      each".
- **llama** (Nelson Elhage, [repo][llama], still maintained, not REAPI) runs
  compiler invocations on Lambda over a content-addressed S3 store.
  - clang+LLVM went from 5:33 to 1:24 at `-j400`, for about $0.49.
  - The bottleneck was data transfer: builds spent "multiple *seconds*"
    uploading to S3 ([blog, 2021-05-20][llama-blog]).
  - Cold starts can "lose all of the gains from concurrency"
    ([2021-03-31][llama-prof]).
- **bazel-cache** (znly, now [SaveTheRbtz/bazel-cache][bzc], dormant since
  2022). A REAPI cache (no `Execute`) "meant to be deployed serverless-ly
  (currently on Cloud Run)" on GCS.
  - A fully cached build took 4.5 s through Cloud Run gRPC, against 10.5 s
    reading GCS directly over HTTP.
  - Garbage collection used a TTL: each read bumps the object's GCS
    `CustomTime`, and a `DaysSinceCustomTime` lifecycle rule deletes the rest.
    This scales to zero.
  - The friction: Cloud Run accepts only identity tokens, so Bazel needed a
    wrapper script to get one.
- **BuildBuddy Firecracker `recycle-runner`** ([docs][bbd-fc]) reuses warm
  VMs as "an optimization, and should not be relied upon for correctness".
  - That is the model for a per-instance cache of blobs and store paths:
    helps when warm, correct when cold *(inferred)*.
- **Google Cloud RBE** is no longer offered: its docs page 404s. No
  first-party announcement was found *(unverified)*.

Lessons for turnkey *(inferred)*:

- Per-action functions pay off when actions last about a second or more.
- Moving data between the store and the executor dominates cost, which
  argues for a per-instance cache.
- A REAPI CAS and AC on Cloud Run over GCS is proven.
- Nobody has published a REAPI `Execute` that runs inside the request.

### Cloud Run facts that bear on a streaming `Execute`

All from docs.cloud.google.com, pages updated 2026-09-24.

- **Request timeout** ([request-timeout][cr-timeout]).
  - The default is 300 s and the maximum is 3,600 s.
  - On timeout the connection is closed with a 504, but the instance is not
    terminated.
- **Streaming gRPC is supported** ([grpc][cr-grpc]). "each stream is counted
  once against the maximum concurrent requests". End-to-end HTTP/2 needs the
  container to serve h2c; Google terminates TLS ([http2][cr-http2]).
- **Concurrency** ([about-concurrency][cr-conc]).
  - The default is 80 per vCPU and the maximum is 1,000.
  - Concurrency 1 gives an action the whole instance for its request, but
    successive actions still share the instance.
- **CPU** ([billing-settings][cr-billing]). With request-based billing, "CPU
  is only allocated during request processing". So output upload and the AC
  write must finish before the `Execute` stream closes *(inferred)*.

## 3. What buck2's RE client needs from the server

Read at the pinned buck2 [`6507dd15`][b2] (release 2026-09-15,
[`nix/buck2/buck2-source.nix`](../../nix/buck2/buck2-source.nix)). All paths
below are relative to that commit.

### RPCs it calls

These are all in `remote_execution/oss/re_grpc/src/client.rs`:

- `GetCapabilities` ([L352-L398][b2-caps])
- `Execute` ([L748-L876][b2-exec])
- `GetActionResult` ([L688-L714][b2-gar])
- `UpdateActionResult` ([L716-L746][b2-uar]), used only for cache uploads and
  the permission probe
- `FindMissingBlobs` ([L981-L1064][b2-fmb])
- `BatchUpdateBlobs` and ByteStream `Write` ([L878-L915][b2-upload])
- `BatchReadBlobs` and ByteStream `Read` ([L942-L979][b2-download])

It never calls **`WaitExecution`** or **`GetTree`**. Neither appears in
`re_grpc/src`.

### Execute

- **One stream per action.**
  - The client reads every `Operation`. For each one that is not yet `done` it
    decodes `ExecuteOperationMetadata.stage`, for progress display only.
  - The `done` operation carries the `ExecuteResponse`
    ([L784-L855][b2-exec-stream]).
  - A stream that ends without an `ExecuteResponse` is a hard error
    ([buck2_execute re/client.rs L1203-L1215][b2-no-resp]).
  - The operation `name` is ignored.
- **Retries cover opening the call only.**
  - Up to 5 attempts on Unavailable, ResourceExhausted, Aborted, Cancelled or
    a transport reset ([L588-L655][b2-retry], [L763-L781][b2-exec-retry]).
  - An error mid-stream is not retried, resubmitted or resumed *(inferred:
    no handler)*.
  - So the whole action must finish within one server-streaming call, which
    is exactly the request-driven shape.
  - It also means Cloud Run scale-in or an instance crash fails the action,
    and the 60-minute request cap is a hard limit on action length
    *(inferred)*.
- **Status and exit code** ([L811-L813][b2-status], [L129-L139][b2-status-conv]).
  - A non-OK `ExecuteResponse.status` is an error even when `result` is
    present.
  - A failing command must therefore come back as status OK with
    `ActionResult.exit_code` set.
  - In OSS, a server-side timeout returned as `DEADLINE_EXCEEDED` is a generic
    error: `is_timeout_error` is false outside fbcode
    ([executors/re.rs L539-L553][b2-timeout-err]).
  - A status message containing `OUTMISS` means missing outputs
    ([L530-L535][b2-outmiss]).
  - `message` is shown to the user ([L816-L826][b2-msg]).
  - `server_logs` is never read.
- **`cached_result`** is used only to label the trace as a cache hit or a
  remote execution ([re/client.rs L1693][b2-cached]).
- **Cache lookup.**
  - buck2 runs its own `GetActionResult` before `Execute` unless caching is
    off ([daemon/common.rs L279-L345][b2-accheck],
    [action_cache.rs L113-L116][b2-accheck2]).
  - `skip_cache_lookup` is true when any of these holds
    ([re/client.rs L1522-L1524][b2-skip], [common.rs L245-L246][b2-skip2]):
    - `skip_remote_cache` is configured
    - `--no-remote-cache` is passed
    - `remote_cache_enabled = false`
    - an induced cache miss is active
  - A server-side AC lookup inside `Execute` is therefore redundant for a
    well-configured buck2. It is still worth honouring when
    `skip_cache_lookup` is false *(inferred)*.
- **`do_not_cache` is always `false`**
  ([command_executor.rs L236-L245][b2-dnc]). The server decides what to cache.
- **`Action.timeout`** is set for tests and for `run` actions that declare a
  timeout ([command_executor.rs L393-L400][b2-timeout],
  [orchestrator.rs L1822-L1823][b2-test-timeout],
  [run.rs L1300-L1302][b2-run-timeout]).
  - The client sets no deadline of its own on `Execute`.

### ActionResult and outputs

- **`execution_metadata` is mandatory.** Without it conversion fails with
  "The execution metadata are not defined"
  ([convert_action_result L1136-L1231][b2-convert]).
  - The timestamps are compared with the action timeout, and a soft error is
    raised when the timeout is exceeded ([re.rs L335-L353][b2-meta-timeout]).
  - `worker` and `auxiliary_metadata` are read.
- **Each output file needs a digest and each output directory a
  `tree_digest`.**
  - `output_paths` and `output_file_symlinks` in the result are ignored.
  - The `Command` lists outputs as `output_files` and `output_directories`, or
    as `output_paths`, depending on `output_paths_behavior`
    ([command_executor.rs L328-L389][b2-outpaths]).
- **`Tree` blobs are downloaded eagerly.** File outputs are only declared to
  the materializer and fetched later ([download.rs L443-L469][b2-tree],
  [L507-L513][b2-declare]).
  - The server must upload the `Tree` for every output directory.
  - It must keep every output blob in CAS, or [buck2#862][b2-862] (a dangling
    AC→CAS reference is a hard error) applies.

### Inputs, instance names, headers, capabilities

- **Inputs are uploaded first.**
  - buck2 calls `FindMissingBlobs` in batches of 100 and uploads what is
    missing ([re.rs L406-L415][b2-inputs]).
  - It then assumes present blobs live for `cas_ttl_secs` (default 3 h), and
    remembers their presence for up to 12 h ([L1052-L1060][b2-ttl],
    [L675-L679][b2-lru]).
  - A server that evicts sooner can receive an `Execute` whose inputs are
    gone. It must fail cleanly, with `FAILED_PRECONDITION` and missing-blob
    details, and buck2 does not re-upload on that error *(inferred)*.
  - GC must respect a floor of about 12 h since the last `FindMissingBlobs`
    touch *(inferred)*.
- **`instance_name`** goes on every request, and as a `{instance}/` prefix on
  ByteStream resource names ([L198-L210][b2-inst], [L1329-L1350][b2-bs-name]).
- **`http_headers`** go on every call, with `$ENV` substitution
  ([L408-L432][b2-headers]). That is how a bearer token (for example a
  GitHub-issued credential, turnkey-wit.6) reaches the server.
- **`RequestMetadata`** carries `tool_name = "buck2"` and
  `tool_invocation_id`. `action_id`, `target_id` and `action_mnemonic` are
  empty ([L1764-L1843][b2-reqmeta]). The server cannot attribute an action to
  a target from metadata.
- **Capabilities** ([L377-L394][b2-caps-read], [L293-L310][b2-compress]).
  - Only `cache_capabilities.max_batch_total_size_bytes` (0 means no limit;
    the default is 4,000,000) and `supported_compressors` (zstd preferred for
    ByteStream) are read.
  - Digest functions, `exec_enabled` and execution capabilities are ignored.
  - `engine_address`, `cas_address` and `action_cache_address` are all
    required ([L313-L317][b2-addrs]).
- **Platform properties** arrive only on `Command.platform`. `Action.platform`
  is empty and `salt` is unset (also in
  [local-re-api-servers.md](local-re-api-servers.md#client-configuration)).

## 4. Building our own: minimal surface and reusable parts

### What a minimal server must implement for buck2

| Service / RPC | Behaviour required | Can come from bb-storage |
|---|---|---|
| `Capabilities.GetCapabilities` | `max_batch_total_size_bytes`, SHA256, optionally zstd | Yes |
| `ContentAddressableStorage.FindMissingBlobs` / `BatchUpdateBlobs` / `BatchReadBlobs` | Verify hashes on upload; `FindMissingBlobs` refreshes the GC clock | Yes (servers, verification); refresh on GCS is ours |
| `ByteStream.Read` / `Write` | Resource names prefixed with the instance; hashes verified; compression optional | Yes |
| `ActionCache.GetActionResult` | Must not return entries whose blobs are gone ([buck2#862][b2-862]) | Yes (`completenesschecking`) |
| `ActionCache.UpdateActionResult` | **Deny to every client** with `PERMISSION_DENIED`. buck2 probes for it and then turns uploads off ([local-re-api-servers.md](local-re-api-servers.md#the-cache-only-shape)) | Yes (`put_authorizer` that denies) |
| `Execution.Execute` | Stream stage updates, run inline, upload outputs and `Tree`s, write the AC, return `ExecuteResponse` with `execution_metadata`; non-zero exit is status OK | **Ours**: a handler around `BuildExecutor` |
| `Execution.WaitExecution` | Not called by buck2; return `UNIMPLEMENTED` or `NOT_FOUND` | — |

### Reusable Go components

- **REAPI protos.** `github.com/bazelbuild/remote-apis/build/bazel/remote/execution/v2`
  ships committed `.pb.go` and `_grpc.pb.go` files ([dir][reapi-go]).
- **bb-storage**, Apache-2.0:
  - `pkg/blobstore/grpcservers`: servers for CAS, AC and ByteStream.
  - `pkg/blobstore/buffer`: hash validation of client uploads.
  - `pkg/digest`
  - `pkg/auth` and `pkg/jwt`: authorizers.
  - `pkg/blobstore/completenesschecking`
  - Storage sits behind `BlobAccess`, a three-method interface (`Get`, `Put`,
    `FindMissing`) ([blob_access.go L15-L21][bb-blobaccess]). A GCS
    implementation is ours to write.
  - Plausible layout: GCS as the backing store behind a per-instance
    `local`/in-memory `readCaching` tier, which answers Buildbarn's reasons
    for removing its cloud backend *(inferred, not prototyped)*.
- **bb-remote-execution**, Apache-2.0:
  - `BuildExecutor.Execute` takes an action and returns an `ExecuteResponse`
    ([build_executor.go L68-L71][bbre-iface]).
  - `NewLocalBuildExecutor` stages the input root, runs the command and
    uploads outputs ([L82][bbre-local]).
  - `NewCachingBuildExecutor` writes the AC ([L36][bbre-caching]).
  - Only `NewBuildClient` ties the executor to the scheduler queue
    ([build_client.go L48][bbre-client]).
  - An `Execute` handler can therefore call the executor chain directly
    *(inferred, not prototyped)*.
  - The runner is reached as a gRPC `RunnerClient` (`NewLocalRunner` is a
    `RunnerServer`, [local_runner.go L129][bbre-runner]). In one process it
    needs a bufconn or loopback adapter, or a separate runner process in the
    same container.
  - The runner is also where Nix store paths get mounted into the sandbox
    (turnkey-wit.3).
  - Bypassing bb_scheduler also bypasses its `Command.platform` bug
    ([buck2#1477][b2-1477]) *(inferred)*.
- **Stability caveat.** Neither Buildbarn module is tagged; the Go proxy
  serves only pseudo-versions such as `v0.0.0-20260924123715-b0032b1d9bee`.
  Pin exact revisions and bump them deliberately.
- **Storage client.** `cloud.google.com/go/storage` or `gocloud.dev/blob`
  (v0.46.0, 2026-06-02). please-servers' Elan is working prior art for a
  REAPI CAS on `gocloud.dev/blob` with hash checking ([rpc.go][ps-verify]).
- **Tests.** remote-apis-sdks `go/pkg/fakes` has a fake CAS, AC and Exec
  ([exec.go][sdk-fakes]).

### Rust

- There is no first-party REAPI crate. `bazel-remote-apis` 0.29.0 (MIT,
  third-party) exists, and NativeLink's crates are unpublished and
  FSL-licensed.
- Generating tonic stubs from the proto revision buck2 pins would work.
  turnkey already fetches buck2's protos (`protos` in
  [`buck2-source.nix`](../../nix/buck2/buck2-source.nix)).
- There is no permissively licensed Rust layer for storage or execution to
  reuse. Rust would mean writing the CAS servers, verification, authorization
  and executor ourselves *(inferred)*.

## 5. Recommendation

**Build on Buildbarn's Go libraries; write the `Execute` handler and the GCS
`BlobAccess` ourselves.**

Why:

1. **Nothing fits as shipped.**
   - The in-request servers (justbuild, buildbox-casd) have no GCS, are C++,
     and are not libraries.
   - The GCS-capable servers are queue-based:
     - NativeLink's scheduler cannot be collapsed, and it is FSL/BUSL.
     - BuildBuddy's GCS storage and execution are enterprise only.
     - BuildGrid needs SQL and bots.
2. **buck2 asks for little.** The server needs:
   - one stream per action
   - no `WaitExecution` and no operation persistence
   - two Capabilities fields
   - its own AC lookup, which buck2 already does

   The request-driven handler is small; justbuild's core is about 50 lines.
3. **Buildbarn already covers the security-relevant parts:**
   - hash-verified uploads
   - `put_authorizer`, which enforces the settled trust rule "clients submit
     work, never results" per store
   - JWT authentication
   - AC→CAS completeness checking
   - an action executor (staging, running, upload) that is separate from its
     scheduler

   All of it is Apache-2.0 and in Go, one of turnkey's two languages.
4. **Writing everything from scratch in Rust** would redo all of that with no
   reusable permissively licensed base.

Risks to carry into turnkey-wit.10 and turnkey-wit.12:

- **GCS latency.** Buildbarn removed its cloud backend over time to first
  byte and `FindMissing` cost. We need a per-instance cache tier, and
  `FindMissingBlobs` must be cheap: batched GCS metadata calls, or an index.
  How fast this gets is unmeasured.
- **Library stability.** Buildbarn has no tags or API promises. Pin exact
  revisions and expect breakage on bumps.
- **The 60-minute cap and no resume.** An action longer than Cloud Run's
  request timeout cannot run, and scale-in or a crash fails the action. Where
  deploys and scale-in drain `Execute` streams, the drain window must be
  about as long as the longest action *(inferred)*.
- **The runner seam.** bb_runner's gRPC runner is the natural place to add
  Nix store-path mounts and per-action isolation. How that works on Cloud Run
  is the subject of turnkey-wit.1 and turnkey-wit.3.

## Unverified or open

- buildbox-casd: whether its AC endpoint can refuse client
  `UpdateActionResult`.
- BuildGrid on GCS through its S3-compatible API.
- GCS strong read-after-write consistency and `ifGenerationMatch` write-once
  semantics: well known, but not re-read for this note.
- Whether bb-storage's `grpcservers` and bb-remote-execution's `builder`
  compose in one process without bb_worker's scaffolding. This is inferred
  from the interfaces and has not been prototyped.
- Latency per action of the whole path (Cloud Run request → stage inputs from
  GCS → run → upload). No published number exists for REAPI. gg reports
  about 140 ms of invocation overhead on Lambda for a different protocol.

## Revisions read

- buck2 `6507dd157a6f81a810c48583edf1758dd0c337c5` (turnkey's pin)
- bb-storage `b0032b1d9beed6f6fac8a209b2ef3b534a695c3b` (2026-09-24)
- bb-remote-execution `c7bbd666d6dbf5a5f85258d7752f2f2254f58160` (2026-09-23)
- NativeLink v1.7.2 `b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6` (2026-09-25)
- bazel-remote v2.6.2 `3cb3084b57542c260860a484afcfe69557f6b27c`
- BuildBuddy v2.310.0 `91e473f2d54a69ad3bf047b41195849b26bf5fd0`
- BuildGrid `d62da23cedcc05cd76da054ffef60e34e93dbde9` (2026-09-22)
- buildbox `efd1bfd17439d0e59f79809e741033d089f3f30f` (2026-09-25)
- justbuild `68cd8d41672a5d2a1d8fadb61d653965b121d3e0` (2026-09-08)
- Bazel Buildfarm `344f3a82` (2026-09-25)
- remote-apis `adbf4a27c86fbea4a37637a6cbcacef372406fe7`
- remote-apis-sdks `d5824b1a2286806b07efd030aa3a139c4f540157`
- please-servers `2bcbbfe` (2026-07-13)

[b2]: https://github.com/facebook/buck2/tree/6507dd157a6f81a810c48583edf1758dd0c337c5
[b2-caps]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L352-L398
[b2-exec]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L748-L876
[b2-gar]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L688-L714
[b2-uar]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L716-L746
[b2-fmb]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L981-L1064
[b2-upload]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L878-L915
[b2-download]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L942-L979
[b2-exec-stream]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L784-L855
[b2-no-resp]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/re/client.rs#L1203-L1215
[b2-retry]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L588-L655
[b2-exec-retry]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L763-L781
[b2-status]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L811-L813
[b2-status-conv]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L129-L139
[b2-timeout-err]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/executors/re.rs#L539-L553
[b2-outmiss]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/executors/re.rs#L530-L535
[b2-msg]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L816-L826
[b2-cached]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/re/client.rs#L1693
[b2-accheck]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_server/src/daemon/common.rs#L279-L345
[b2-accheck2]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/executors/action_cache.rs#L113-L116
[b2-skip]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/re/client.rs#L1522-L1524
[b2-skip2]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_server/src/daemon/common.rs#L245-L246
[b2-dnc]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L236-L245
[b2-timeout]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L393-L400
[b2-test-timeout]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_test/src/orchestrator.rs#L1822-L1823
[b2-run-timeout]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_action_impl/src/actions/impls/run.rs#L1300-L1302
[b2-convert]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L1136-L1231
[b2-meta-timeout]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/executors/re.rs#L335-L353
[b2-outpaths]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/command_executor.rs#L328-L389
[b2-tree]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/re/download.rs#L443-L469
[b2-declare]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/re/download.rs#L507-L513
[b2-inputs]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute_impl/src/executors/re.rs#L406-L415
[b2-ttl]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L1052-L1060
[b2-lru]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L675-L679
[b2-inst]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L198-L210
[b2-bs-name]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L1329-L1350
[b2-headers]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L408-L432
[b2-reqmeta]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L1764-L1843
[b2-caps-read]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L377-L394
[b2-compress]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L293-L310
[b2-addrs]: https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L313-L317
[b2-862]: https://github.com/facebook/buck2/issues/862
[b2-1477]: https://github.com/facebook/buck2/issues/1477
[jb-exec]: https://github.com/just-buildsystem/justbuild/blob/68cd8d41672a5d2a1d8fadb61d653965b121d3e0/src/buildtool/execution_api/execution_service/execution_server.cpp#L268-L318
[jb-ac]: https://github.com/just-buildsystem/justbuild/blob/68cd8d41672a5d2a1d8fadb61d653965b121d3e0/src/buildtool/execution_api/execution_service/ac_server.cpp#L64-L72
[jb-cas]: https://github.com/just-buildsystem/justbuild/blob/68cd8d41672a5d2a1d8fadb61d653965b121d3e0/src/buildtool/execution_api/execution_service/cas_utils.cpp#L70-L74
[casd-sched]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/efd1bfd17439d0e59f79809e741033d089f3f30f/casd/buildboxcasd_localexecutionscheduler.h
[casd-verify]: https://gitlab.com/BuildGrid/buildbox/buildbox/-/blob/efd1bfd17439d0e59f79809e741033d089f3f30f/casd/buildboxcasd_localcasinstance.cpp#L1624
[bb-cloud-removed]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/blobstore/blobstore.proto#L224-L245
[bb-authz]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/bb_storage/bb_storage.proto#L115-L147
[bb-jwt]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/proto/configuration/jwt/jwt.proto#L18-L29
[bb-cas-verify]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/grpcservers/content_addressable_storage_server.go#L137
[bb-bs-verify]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/grpcservers/byte_stream_server.go#L183
[bb-blobaccess]: https://github.com/buildbarn/bb-storage/blob/b0032b1d9beed6f6fac8a209b2ef3b534a695c3b/pkg/blobstore/blob_access.go#L15-L21
[bbre-iface]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/builder/build_executor.go#L68-L71
[bbre-local]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/builder/local_build_executor.go#L82
[bbre-caching]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/builder/caching_build_executor.go#L36
[bbre-client]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/builder/build_client.go#L48
[bbre-runner]: https://github.com/buildbarn/bb-remote-execution/blob/c7bbd666d6dbf5a5f85258d7752f2f2254f58160/pkg/runner/local_runner.go#L129
[nl-sched]: https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/schedulers.rs#L30-L36
[nl-worker-ep]: https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/cas_server.rs#L958-L967
[nl-gcs]: https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/stores.rs#L1220-L1300
[nl-ro]: https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/cas_server.rs#L124
[nl-verify]: https://github.com/TraceMachina/nativelink/blob/b9d01f2c3e3f9108a207e6e6ad70dd35d50b0db6/nativelink-config/src/stores.rs#L1115
[bbd-tree]: https://github.com/buildbuddy-io/buildbuddy/tree/91e473f2d54a69ad3bf047b41195849b26bf5fd0/enterprise/server/backends
[bbd-acw]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/remote_cache/action_cache_server/action_cache_server.go#L375
[bbd-bs]: https://github.com/buildbuddy-io/buildbuddy/blob/91e473f2d54a69ad3bf047b41195849b26bf5fd0/server/remote_cache/byte_stream_server/byte_stream_server.go#L759
[bbd-fc]: https://www.buildbuddy.io/docs/rbe-microvms/
[bg-storage]: https://gitlab.com/BuildGrid/buildgrid/-/tree/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/cas/storage
[bg-allow]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/app/settings/parser.py#L1767
[bg-verify]: https://gitlab.com/BuildGrid/buildgrid/-/blob/d62da23cedcc05cd76da054ffef60e34e93dbde9/buildgrid/server/cas/instance.py#L242
[br-auth]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/main.go#L170-L260
[br-verify]: https://github.com/buchgr/bazel-remote/blob/3cb3084b57542c260860a484afcfe69557f6b27c/cache/disk/disk.go#L378-L381
[ps-dual]: https://github.com/thought-machine/please-servers/blob/2bcbbfe/mettle/main.go#L165-L185
[ps-ac]: https://github.com/thought-machine/please-servers/blob/2bcbbfe/elan/rpc/rpc.go#L290-L303
[ps-verify]: https://github.com/thought-machine/please-servers/blob/2bcbbfe/elan/rpc/rpc.go#L760-L775
[reapi-go]: https://github.com/bazelbuild/remote-apis/tree/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2
[sdk-fakes]: https://github.com/bazelbuild/remote-apis-sdks/blob/d5824b1a2286806b07efd030aa3a139c4f540157/go/pkg/fakes/exec.go
[hc-infra]: https://github.com/hchauvin/bazel-cloud-infra
[gg-paper]: https://www.usenix.org/system/files/atc19-fouladi.pdf
[gg-repo]: https://github.com/StanfordSNR/gg
[llama]: https://github.com/nelhage/llama
[llama-blog]: https://blog.nelhage.com/post/building-llvm-in-90s/
[llama-prof]: https://buttondown.com/nelhage/archive/profiling-llama/
[bzc]: https://github.com/SaveTheRbtz/bazel-cache
[cr-timeout]: https://docs.cloud.google.com/run/docs/configuring/request-timeout
[cr-grpc]: https://docs.cloud.google.com/run/docs/triggering/grpc
[cr-http2]: https://docs.cloud.google.com/run/docs/configuring/http2
[cr-conc]: https://docs.cloud.google.com/run/docs/about-concurrency
[cr-billing]: https://docs.cloud.google.com/run/docs/configuring/billing-settings
