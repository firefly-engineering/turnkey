# Can Cloud Run host an REAPI executor and a Nix builder?

Research for **turnkey-wit.1**, part of map turnkey-wit (incremental builds
everywhere). Gathered on 2026-09-26 from Google's Cloud Run documentation,
release notes and pricing pages, Docker's seccomp profile, and Nix's source.
Every Cloud Run documentation page cited here showed "Last updated
2026-09-24". Claims are sourced inline. *(inferred)* marks a conclusion drawn
from sources rather than stated in them. *(unverified)* marks something no
primary source settles and that needs an experiment.

The question has three parts:

1. Can a Cloud Run service be an REAPI executor, running each buck2 action
   inside its own `Execute` request?
2. Can Cloud Run build client-submitted Nix derivations?
3. Can both scale to zero?

The map's choice between results **shared across all clients** and the
fallback of **one service per trust level** turns on one question: can one
action tamper with the next action that runs on the same reused instance?

## TL;DR

- **Execute over gRPC works on services.**
  - Cloud Run services support every gRPC mode, server streaming included,
    once end-to-end HTTP/2 (h2c) is enabled.
  - A request can last at most 60 minutes. With HTTP/2 there is no
    request-size limit.
  - An action's CPU is only guaranteed while its `Execute` stream is open,
    so each action must finish inside its request.
- **Instances are reused, and nothing forces a fresh one per request.**
  - Services send requests to already-running instances. The only knob is
    concurrency=1, which serialises requests; it does not recycle the
    instance.
  - Only **jobs** get a fresh instance each time, with one task per
    instance.
- **Verdict: successive actions on one instance *can* be isolated, with
  Cloud Run sandboxes.** These are a Preview feature, released
  2026-07-08. `sandbox do` gives each command:
  - a fresh tmpfs overlay, thrown away when it exits;
  - a read-only view of the host container's filesystem;
  - read-only or read-write bind mounts;
  - no environment variables, no secrets, no metadata server, and no network
    egress by default.

  Google documents that "all sandboxes are completely isolated from each
  other". That is exactly the per-action sandbox the shared-results model
  needs, so **the map's shared-results default can stand**. Three
  conditions come with it:
  - the feature is Pre-GA;
  - the isolation mechanism behind it is not documented;
  - it has not yet been tried with a real action *(unverified)*.
- **Rolling our own sandbox (bubblewrap, nsjail, the Nix sandbox) is not
  documented to work.** The docs contradict each other:
  - the gen2 pages promise "support for all system calls, namespaces, and
    cgroups";
  - the container contract applies "basic security profiles, similar to
    seccomp security profiles for Docker", grants no host root
    capabilities, and says mounting from inside containers is unsupported;
  - Docker's default profile blocks `unshare` and namespace-creating
    `clone` without `CAP_SYS_ADMIN`.

  Nix's Linux sandbox needs exactly those calls. This needs an experiment.
- **The Nix builder does not fit a request for long builds.**
  - A build longer than 60 minutes must run as a Cloud Run **job**: tasks
    run up to 7 days, each task on its own instance.
  - Short builds can run in a service inside a Cloud Run sandbox.
  - Whether `nix build` with `sandbox = true` works at all is
    *(unverified)*.
  - Nix silently turns its sandbox off when the kernel refuses
    (`sandbox-fallback`, true by default), so the builder must set
    `sandbox-fallback = false`.
- **Scaling to zero holds.**
  - With request-based billing and min-instances=0, idle instances are not
    charged.
  - An ephemeral disk is billed only while its instance lives.
  - At rest the bill is stored bytes: GCS, plus the executor image in
    Artifact Registry. Traffic between Cloud Run and GCS in the same region
    is free.
- **Contradictions with the map Notes:**
  - "functions or services": only services (or jobs) fit.
  - "built in the Nix sandbox" is not yet shown to be possible on Cloud Run.

  See [Contradictions with settled decisions](#contradictions-with-settled-decisions).

## Fit table

"Services" means Cloud Run services on the gen2 execution environment unless
noted.

| # | Requirement | Supported? | Source |
|---|---|---|---|
| 1 | gRPC server streaming (`Execute` returns a stream of `Operation`) | **Yes.** "all gRPC types, streaming or unary". Each stream counts once against the instance's concurrency | [gRPC][cr-grpc]; GA 2021-06-22 ([release notes][cr-relnotes]) |
| 1 | HTTP/2 end-to-end | **Yes, opt-in.** Without it, Cloud Run downgrades to HTTP/1 "with the exception of native gRPC traffic". With it, the container must speak h2c | [HTTP/2][cr-http2], [contract][cr-contract] |
| 1 | Max request duration | **60 min** for services; default 5 min | [quotas][cr-quotas], [request timeout][cr-timeout] |
| 1 | Max request duration, functions | 60 min for HTTP functions; **9 min** for event-driven functions | [functions comparison][cr-fn] |
| 1 | Max request/response size | HTTP/1: 32 MiB each. HTTP/2: "No limit" on requests; the response limit does not apply when streaming | [quotas][cr-quotas] |
| 1 | Streams per connection | 100 concurrent streams per HTTP/2 client connection | [quotas][cr-quotas] |
| 1 | Idle timeout on long inbound streams | **Not documented** beyond the request timeout. The documented idle timeouts (10 min to VPC, 20 min to internet) are for *outbound* connections | [contract][cr-contract] |
| 1 | `WaitExecution` reaching the instance that runs the action | **No routing guarantee** *(inferred)*. Requests go to whichever instance the load balancer picks, so operation state must live outside the instance | [autoscaling][cr-autoscaling] |
| 2 | Max vCPU / memory per instance | **8 vCPU, 32 GiB** | [quotas][cr-quotas]; GA 2023-05-17 ([release notes][cr-relnotes]) |
| 2 | Writable root filesystem | Yes. **In-memory**, counted against instance memory, no size limit, lost when the instance stops | [contract][cr-contract] |
| 2 | In-memory volume (emptyDir) | Yes (GA 2024-11-12). Memory-backed; the size limit only caps it | [in-memory volumes][cr-inmem] |
| 2 | Ephemeral disk | **Preview** (2026-04-20). ext4, 1–100 GiB per volume, 10 GiB per instance by default. Per-region quota of 100 GiB, which must cover *disk size × max instances*. Gen2 only. Billed for the instance's lifetime | [ephemeral disk][cr-ephdisk], [pricing][cr-pricing] |
| 2 | Cloud Storage FUSE volume | Yes (gen2). "not a fully POSIX-compliant file system", no concurrency control for multiple writers. Scales to zero (you pay for GCS) | [GCS volumes][cr-gcsfuse] |
| 2 | NFS / Filestore volume | Yes (gen2, VPC needed). Filestore is a provisioned server, so it does **not** scale to zero *(inferred)* | [NFS volumes][cr-nfs] |
| 3 | User, mount and PID namespaces inside the container | **Conflicting docs** *(unverified)*. Gen2 offers "support for all system calls, namespaces, and cgroups". The contract grants no host root capabilities, applies Docker-like seccomp profiles, and does not support mounting from inside containers | [environments][cr-envs], [contract][cr-contract], [Docker profile][moby-seccomp] |
| 3 | Per-action sandbox inside one instance | **Yes, Preview: Cloud Run sandboxes** (services 2026-07-08; jobs and worker pools 2026-08-05) | [code execution][cr-code-exec], [release notes][cr-relnotes] |
| 3 | Non-root uid | Yes. The image's `USER` is used; root only if the user does not exist. setuid and sudo are unsupported (root then `su` works). Inside a sandbox, processes run "with sudo privileges as a non-root user" | [contract][cr-contract], [code execution][cr-code-exec] |
| 3 | seccomp | "Basic security profiles, similar to seccomp security profiles for Docker". No eBPF. No writes to most of `/dev`, `/proc`, `/sys` | [contract][cr-contract] |
| 3 | Fresh instance per request (services) | **No.** Instances are reused until idle for up to 15 min. There is no max-requests-per-instance setting; concurrency=1 only serialises | [autoscaling][cr-autoscaling], [concurrency][cr-concurrency] |
| 3 | Fresh instance per unit of work (jobs) | **Yes.** "each task running one instance"; "all new instances are started from a clean slate" | [resource model][cr-resmodel], [security][cr-security] |
| 4 | `nix build` with `sandbox = true` on Cloud Run | **No evidence either way** *(unverified)*. Nix's Linux sandbox needs `clone(CLONE_NEWNS \| CLONE_NEWPID \| …)` and ideally `CLONE_NEWUSER` | [Nix source][nix-clone] |
| 4 | `/nix/store` location | In the in-memory root filesystem, an in-memory volume, or an ephemeral disk. A GCS FUSE store is not POSIX-complete *(inferred: poor fit)* | [contract][cr-contract], [GCS volumes][cr-gcsfuse] |
| 5 | Scale to zero | **Yes, the default.** min-instances defaults to 0 | [autoscaling][cr-autoscaling], [contract][cr-contract] |
| 5 | Idle cost | **Zero** with request-based billing: "Idle instances that are not minimum instances are not charged" | [pricing][cr-pricing] |
| 5 | CPU outside requests | Only with instance-based billing. With request-based billing, "CPU is only allocated during request processing" | [billing][cr-billing], [contract][cr-contract] |
| 5 | Max instances | 100 per revision by default; the ceiling is bound by regional CPU and memory quotas | [max instances][cr-maxinst], [quotas][cr-quotas] |
| 5 | Concurrency | 1–1000 per instance. Default 80 × vCPU with gcloud or Terraform | [concurrency][cr-concurrency] |
| 5 | Cold start for a large image | **No figure published.** Images are pulled with "container image streaming". Startup CPU boost exists. Instances must listen within 4 min | [tips][cr-tips], [contract][cr-contract] |
| 6 | Jobs: task timeout | Default 10 min, **max 168 h** (GA 2025-11-11); 1 h with GPUs | [task timeout][cr-tasktimeout], [quotas][cr-quotas] |
| 6 | Jobs: parallelism and task count | Up to 10,000 tasks per execution. Parallelism defaults to at most 100 and depends on the region's quota | [quotas][cr-quotas], [resource model][cr-resmodel] |
| 6 | Jobs: start latency | **Not published** | — |
| 7 | Cost at load | Request-based: $0.000024/vCPU-s, $0.0000025/GiB-s, $0.40 per million requests. Instance-based and jobs: $0.000018/vCPU-s, $0.000002/GiB-s. Ephemeral disk: $0.000109589/GiB-hour | [pricing][cr-pricing] |
| 7 | Cloud Run ↔ GCS in the same region | **Free** in both directions | [Cloud Run pricing][cr-pricing], [GCS pricing][gcs-pricing] |
| 7 | Cost at zero | Stored bytes: GCS Standard ≈ $0.000027397/GiB-hour (≈ $0.02/GiB-month) for the page's default region, plus operations. Container images in Artifact Registry are also stored bytes *(inferred)* | [GCS pricing][gcs-pricing] |

The free tier also applies. With request-based billing it covers 180,000
vCPU-s, 360,000 GiB-s and 2 million requests a month.

## 1. Request model

- **Timeout.** "The timeout is set by default to 5 minutes (300 seconds) and
  can be extended up to 60 minutes (3600 seconds)"
  ([request timeout][cr-timeout]). The 60-minute limit has been GA since
  2021-06-03 ([release notes][cr-relnotes]).
  - One `Execute` stream is one request, so **an action can run for at most
    60 minutes** *(inferred)*.
  - buck2's default action timeouts sit well below that. Long tests would
    need a job-backed path.
- **gRPC and streaming.**
  - "You can use all gRPC types, streaming or unary, with Cloud Run", and
    "each stream is counted once against the maximum concurrent requests"
    ([gRPC][cr-grpc]).
  - Streaming needs HTTP/2 end to end: "many gRPC features, such as
    streaming and metadata, require HTTP/2" ([gRPC][cr-grpc]).
  - With HTTP/2 end to end, "your container must handle requests in HTTP/2
    cleartext (h2c) format, because TLS is still terminated automatically by
    Cloud Run" ([contract][cr-contract]).
  - WebSockets, HTTP/2 and gRPC streaming went GA on 2021-06-22
    ([release notes][cr-relnotes]).
- **Size limits.**
  - Maximum HTTP/1 request size is 32 MiB, and the "limit applies if using
    HTTP/1 server. No limit if using HTTP/2 server."
  - The 32 MiB response limit does not apply "if … using `Transfer-Encoding:
    chunked` or streaming" ([quotas][cr-quotas]).
  - CAS uploads through `ByteStream.Write`, a client stream, are therefore
    unbounded on HTTP/2 *(inferred)*.
- **Streams per connection.** 100 concurrent streams per HTTP/2 client
  connection ([quotas][cr-quotas]). A buck2 client with more than 100
  actions in flight needs several channels, or its extra streams queue
  *(inferred)*.
- **`WaitExecution`.** Nothing pins a follow-up request to the instance that
  runs the action: requests are routed by the autoscaler and load balancer
  ([autoscaling][cr-autoscaling]).
  - An executor that supports `WaitExecution`, or reconnects after a
    dropped stream, must keep operation state outside the instance
    *(inferred)*.
  - The alternative is to answer `NOT_FOUND`, which the REAPI permits and
    which makes clients re-`Execute` *(inferred; not checked against buck2
    in this note)*.

## 2. Resources and storage

- **Instance size.** Up to 8 vCPU and 32 GiB per instance
  ([quotas][cr-quotas]).
- **Root filesystem.** "Each container's file system is writable … It is an
  in-memory file system, so writing to it uses the instance's memory …
  You cannot specify a size limit for this file system"
  ([contract][cr-contract]).
- **Volumes.** Each option, and whether its cost scales to zero:
  - *In-memory volume*: capped at a size limit, but it "does not allocate
    additional space"; data "consumes the memory you configured"
    ([in-memory volumes][cr-inmem]). **Scales to zero.**
  - *Ephemeral disk* (**Preview**, 2026-04-20), per
    [ephemeral disk][cr-ephdisk]:
    - disks are "pre-formatted to `ext4`, and encrypted with
      instance-specific keys", and deleted at shutdown;
    - 1–100 GiB per volume, 10 GiB per instance by default;
    - the project starts at 0 GiB per region and is granted 100 GiB on
      first use;
    - "the combined ephemeral disk size per instance multiplied by its
      maximum number of instances cannot exceed the regional allocation
      limit";
    - gen2 only;
    - billed per GiB-hour of provisioned size over the instance's lifetime
      ([pricing][cr-pricing]).

    **Scales to zero.** The quota rule caps fan-out, though: with the
    default 100 GiB, a 10 GiB `/nix/store` cache allows 10 instances.
  - *Cloud Storage FUSE*: "not a fully POSIX-compliant file system", with
    no concurrency control between writers ([GCS volumes][cr-gcsfuse]).
    **Scales to zero.** It suits bulk artifacts, not a live `/nix/store`
    *(inferred)*.
  - *NFS / Filestore*: needs a VPC-reachable NFS server ([NFS][cr-nfs]).
    Filestore is a provisioned instance, so it does **not** scale to zero
    *(inferred; Filestore pricing not read)*.
- **Implication for the programmable base image** *(inferred)*.
  - The per-instance store-path cache from the map Notes lives in instance
    memory or on an ephemeral disk.
  - Either way it is lost when the instance scales in.
  - The instance-local cache is therefore only a warm cache. The Nix binary
    cache in GCS stays the source of truth.

## 3. Isolation

### What the platform guarantees between instances

- "Each Cloud Run instance is protected (sandboxed) from every other by a
  boundary enforced by a virtual machine monitor (VMM)."
- Both generations use "two layers of sandboxing consisting of a
  hardware-backed layer equivalent to individual VMs (x86 virtualization)
  and a software kernel layer".
- Gen2 adds "seccomp system call filtering and Sandbox2 Linux namespaces".

All three are from [security design][cr-security].

- Gen1 is gVisor. Gen2 is "a microVM and provides full Linux compatibility
  rather than system call emulation", with "support for all system calls,
  namespaces, and cgroups" ([environments][cr-envs-config]).
- Jobs always use gen2 ([environments][cr-envs]).
- Sandboxes, volume mounts and ephemeral disks all require gen2.

### What happens between requests on one instance

- Instances are reused. The autoscaler routes requests "to the recommended
  instances first", and idle instances stay up for up to 15 minutes
  ([autoscaling][cr-autoscaling], [contract][cr-contract]).
- Concurrency can be set to 1 "if your container relies on global state that
  two requests cannot share" ([concurrency][cr-concurrency]). That only
  serialises requests; the next request lands on the same, already-used
  instance.
- **No documented setting gives a service a fresh instance per request.**
  "All new instances are started from a clean slate"
  ([security][cr-security]) holds only for *new* instances.
- Jobs are the exception: "Every job execution executes a number of tasks in
  parallel, with each task running one instance"
  ([resource model][cr-resmodel]).

So without an in-instance sandbox, **action N+1 inherits whatever action N
left behind**: files in the writable root filesystem, and background
processes if the executor does not kill the process tree *(inferred)*.

### Rolling our own sandbox (bubblewrap, nsjail, the Nix sandbox)

Unprivileged sandboxes need to create a user namespace, then mount, PID and
network namespaces inside it. The container contract says:

- "Your containers are not granted the root capabilities of the instance's
  security boundary … Privilege escalation is also disabled … Use of sudo and
  setuid binaries are not supported."
- "Cloud Run executes your containers under user, network, PID, and other
  Linux namespaces … prevents your root containers from running as 'true
  root.'"
- "Other mechanisms of mounting from inside user containers are not
  supported and won't work", listing network file systems and "system calls
  that would perform equivalent actions".
- "Basic security profiles, similar to seccomp security profiles for Docker,
  are applied around each of your containers."

All four are from [contract][cr-contract]. Docker's default profile allows
`unshare`, `setns`, `mount` and `clone3` only with `CAP_SYS_ADMIN`. Without
it, `clone` is allowed only when none of the `CLONE_NEW*` flags is set
(mask `0x7E020000`) ([moby/profiles default.json][moby-seccomp]).

If Cloud Run's profile really is Docker-like, then bwrap, nsjail and Nix's
sandbox all fail with `EPERM`. The gen2 page's "support for all system calls,
namespaces" points the other way. **No primary source settles it**
*(unverified)*. The only report found is a dev.to post asserting that
`unshare` works on gen2 jobs; it shows no output, so it is not relied on.

**Experiment to settle it.** Deploy a gen2 service and run, as root and as a
non-root user:

- `unshare -Ur true`
- `unshare -rm --fork true`
- `bwrap --unshare-all --dev-bind / / true`
- `nix build --option sandbox true --option sandbox-fallback false` on a
  trivial derivation

### Cloud Run sandboxes (Preview)

Google's own answer to per-request isolation. The feature is enabled with
`--sandbox-launcher` and forces gen2. It adds `/usr/local/gcp/bin/sandbox`
to the container ([sandboxes][cr-sandboxes]). From
[code execution][cr-code-exec]:

- `sandbox do` "Spins up a sandbox environment … Executes the command you
  specify … Deletes the sandbox after successful execution."
- "Sandboxes isolate process execution. By default, sandboxes don't have
  access to the parent workload, environment variables, secrets, or the
  Google Cloud metadata server. All sandboxes are completely isolated from
  each other."
- Filesystem:
  - "processes you execute inside the sandbox have read-only access to the
    host container root file system";
  - `--write` gives "a temporary file system (tmpfs) overlay … the writes
    will be lost when the sandbox is deleted";
  - `--mount type=bind,source=…,destination=…[,readonly]` shares host
    directories.
- Network: "By default, all outbound traffic from the sandbox is blocked";
  `--allow-egress` opens it.
- Environment: sandboxes "don't inherit environment variables from the host
  container"; `--env` sets them.
- Resources: sandboxes share "its allocated CPU and memory" with the host
  container ([sandboxes][cr-sandboxes]).
- Speed: the launch blog reports starting and stopping 1,000 sandboxes at
  about 500 ms average latency ([blog, 2026-07-10][cr-sandbox-blog]).

The feature went to Preview for services on 2026-07-08, and for jobs and
worker pools on 2026-08-05 ([release notes][cr-relnotes]). It is covered by
the Pre-GA terms: "available 'as is' and might have limited support".

**How an executor would use it** *(inferred)*:

1. Stage the action's input root in a host directory, for example
   `/work/<id>`.
2. Keep the per-instance store-path cache under `/nix/store` on the host.
3. Run the command with `sandbox do` and these options:
   - `--mount …/work/<id>,destination=<exec root>` read-write;
   - `/nix/store` visible read-only, through the root filesystem view or a
     `readonly` bind mount;
   - no `--allow-egress`;
   - `--env` for exactly the `Command.environment_variables`.
4. Read the outputs from `/work/<id>` after the sandbox exits, hash them,
   upload them, and write the `ActionResult`.
5. Delete `/work/<id>`.

With this pattern:

- the action never holds a credential, since the metadata server is
  unreachable;
- the action cannot write anything outside its own directory;
- the action cannot outlive `sandbox do`.

**Verdict on "can successive actions on one instance be isolated":** yes, as
documented, with Cloud Run sandboxes. Isolation rests on the executor, not
on the action. Residual risks:

- **Pre-GA.** The feature can change or disappear.
- **Mechanism undocumented.** The docs claim isolation but do not say
  whether a separate kernel, gVisor or namespaces enforce it. Kernel-level
  escape resistance is therefore unknown.
- **Read-only view of the host root filesystem.** A sandbox can *read*
  other in-flight actions' work directories. Confidentiality is not a goal
  in the map, so this is acceptable, and it cannot tamper with them. The
  executor must keep no secrets in files; it should use the metadata server,
  which sandboxes cannot reach.
- **Shared CPU and memory.** A hostile action can only exhaust its own
  instance; with concurrency=1 that affects only itself.
- **Not yet tried with real actions** *(unverified)*. The experiment above
  should also run `sandbox do` with an action that leaves a daemon behind,
  and check that the process is gone.

If sandboxes turn out unusable, the per-action alternatives are poor. A job
per action gives a fresh instance each time, but job start latency is
unpublished, and it breaks the synchronous `Execute` stream *(inferred)*.
That leaves the map's fallback: **one service per trust level, writing into
requester scopes.**

## 4. Nix inside Cloud Run

- **Sandbox requirements.**
  - Nix's Linux builder starts the build with `CLONE_NEWPID | CLONE_NEWNS |
    CLONE_NEWIPC | CLONE_NEWUTS`, plus `CLONE_NEWNET` for sandboxed
    derivations and `CLONE_NEWUSER` when user namespaces work
    ([linux-derivation-builder.cc L575-L579][nix-clone]).
  - Without a user namespace it needs a build user. Otherwise it fails with
    "cannot perform a sandboxed build because user namespaces are not
    enabled" ([L636-L641][nix-nouserns]).
  - Detection probes `/proc/sys/user/max_user_namespaces`,
    `/proc/sys/kernel/unprivileged_userns_clone`, and a trial
    `clone(CLONE_NEWUSER)` ([linux-namespaces.cc L14-L51][nix-userns-detect]).
  - The manual (2.35.2) says: "The use of a sandbox requires that Nix is run
    as root (so you should use the 'build users' feature …)"
    ([conf-file][nix-conf]).
- **Silent fallback.** `sandbox-fallback` defaults to `true`: "Whether to
  disable sandboxing when the kernel doesn't allow it"
  ([local-settings.hh L418-L419][nix-fallback]). On Cloud Run, an
  unconfigured builder would quietly build unsandboxed. The service's
  builder must set `sandbox-fallback = false` so that failure is loud
  *(inferred)*.
- **Single-user vs daemon.**
  - Cloud Run runs the container as root when the image names no user, and
    `su` works where setuid does not ([contract][cr-contract]).
  - A daemon is unnecessary inside a one-build-per-request builder: a
    single-user, root `nix` with `build-users-group = nixbld` and
    `sandbox = true` is the textbook configuration, *if* the namespace calls
    are allowed *(unverified)*.
- **Evidence it works.** No primary source or public project shows
  `nix build` with `sandbox = true` on Cloud Run gen2. The search turned up
  only projects that *build images for* Cloud Run with Nix.
- **`/nix/store` location.**
  - The store needs a POSIX filesystem that supports hard links and
    permission changes. That rules out GCS FUSE *(inferred)*.
  - The in-memory root filesystem works but eats memory.
  - An ephemeral disk (Preview, at most 100 GiB per volume and subject to
    the regional quota) is the natural home.
  - Everything is lost on scale-in, so outputs must be pushed to the binary
    cache before the request ends.
- **If the Nix sandbox is refused.** Two workarounds, both *(inferred)*:
  - (a) Run each derivation build with `sandbox = false`, inside a Cloud
    Run sandbox:
    - no egress, except for fixed-output derivations;
    - `/nix/store` read-only;
    - `$out` written through a bind mount;
    - the host registers and signs the output.

    This recreates Nix's isolation with Cloud Run's, but loses Nix's
    private `/nix/store` view: a build can read undeclared store paths on
    the host.
  - (b) Run each derivation build as a Cloud Run **job** task. That gives
    a fresh microVM per task, whose isolation is the VMM boundary. It adds
    unknown start latency, and is the natural home for long builds anyway.

## 5. Scaling and CPU

- **Scale from and to zero.**
  - "When a revision does not receive any traffic, by default, it is scaled
    to zero instances."
  - "On-demand scaling is the only driver for scaling from zero."
  - Queued requests wait "up to 10 seconds or 3.5 times the predicted cold
    start time (whichever is higher)".

  All three are from [autoscaling][cr-autoscaling].
- **Startup.** Instances must listen within 4 minutes; images are pulled
  with "container image streaming technology"; startup CPU boost is
  available ([contract][cr-contract], [tips][cr-tips]). Google publishes
  **no cold-start figure** for large images. Gen2 has "longer cold start
  times" than gen1 ([environments][cr-envs]).

  The executor image itself can stay small, because toolchains come from
  the Nix cache per action. The cost moves to the first fetch of each
  store-path closure on a fresh instance *(inferred)*.
- **Concurrency.** Settable from 1 to 1000; the default is 80 × vCPU with
  gcloud or Terraform ([concurrency][cr-concurrency]). Adaptive concurrency
  tuning can lower the effective limit when CPU runs above 90%
  ([autoscaling][cr-autoscaling]).
- **Max instances.** "By default, Cloud Run revisions are configured to
  scale up to a maximum of 100 instances" ([max instances][cr-maxinst]).
  Regional CPU and memory quotas bound the maximum ([quotas][cr-quotas]).
- **CPU outside requests.**
  - With request-based billing (the default), "CPU is only allocated during
    request processing" ([billing][cr-billing]).
  - Instance-based billing allocates CPU for the instance's whole life, and
    bills it, until the instance has been idle up to 15 minutes.
  - **Background work (store-cache GC, async uploads) must therefore happen
    inside a request**, or pay instance-based billing *(inferred)*.
  - The `ActionResult` write and the output uploads must complete before
    the `Execute` stream closes.
- **Shutdown.** SIGTERM, then 10 s, then SIGKILL; "In exceptional cases,
  Cloud Run might initiate a shutdown and send a SIGTERM signal to a
  container that is still processing requests" ([contract][cr-contract]).
  The executor must treat an interrupted action as failed, never as cached
  *(inferred)*.

## 6. Long work: Cloud Run jobs

- Task timeout: default 10 min, up to 168 h (7 days); "the timeout setting
  applies to each attempt of a task" ([task timeout][cr-tasktimeout]). This
  went GA on 2025-11-11 ([release notes][cr-relnotes]).
- Tasks: up to 10,000 per execution, and up to 10 retries. Parallelism
  "Depends on selected region and CPU and memory configurations"
  ([quotas][cr-quotas]); "By default, tasks execute in parallel up to a
  maximum of 100" ([resource model][cr-resmodel]).
- One task, one instance ([resource model][cr-resmodel]), and a new instance
  starts from a clean slate ([security][cr-security]). **This is the
  strongest isolation Cloud Run offers**, and it needs no Preview feature.
- CPU is allocated for the task's whole life ([contract][cr-contract]).
- Jobs support ephemeral disks, sandboxes, GCS and NFS volumes.
- **Start latency is not published.** It needs measuring before choosing
  jobs for anything interactive.
- A job is started through the Admin API, not a request/response. A builder
  that uses jobs is asynchronous: submit, poll, fetch from the cache
  *(inferred)*. That fits a "client submits a derivation, later substitutes
  its output" protocol.

## 7. Cost

From [Cloud Run pricing][cr-pricing], USD for the Tier 1 regions:

| Mode | vCPU-second | GiB-second | Requests | Idle |
|---|---|---|---|---|
| Services, request-based (default) | $0.000024 | $0.0000025 | $0.40 per million | "Idle instances that are not minimum instances are not charged" |
| Services, instance-based | $0.000018 | $0.000002 | — | Charged while the instance lives |
| Jobs | $0.000018 | $0.000002 | — | n/a |
| Ephemeral disk | $0.000109589 per GiB-hour, provisioned size × instance lifetime | | | |

- **Free tier** for request-based billing: 180,000 vCPU-s, 360,000 GiB-s
  and 2 million requests a month.
- **Egress.** "There is no charge for data transfer to Google Cloud resources
  in the same region" ([Cloud Run pricing][cr-pricing]). GCS reads within
  "the same location" are free ([GCS pricing][gcs-pricing]).
- **At rest**, the bill is GCS storage (Standard ≈ $0.000027397/GiB-hour for
  the page's default region), Class A/B operations, and the stored
  executor/builder images *(inferred)*. The scale-to-zero requirement holds.
- **Worked example** *(inferred arithmetic)*: one 4 vCPU / 8 GiB action
  running 30 s costs 4·30·0.000024 + 8·30·0.0000025 ≈ **$0.0035**.

## Blockers and workarounds

| Blocker | Severity | Workaround |
|---|---|---|
| Per-action isolation on a reused instance relies on **Cloud Run sandboxes, which are Pre-GA** | High: decides shared vs per-trust-level results | Adopt it behind the executor's own seam. Run the isolation experiment in §3 before committing. Fall back to per-trust-level services if it fails or regresses |
| Nested namespaces (bwrap, nsjail, the Nix sandbox) **not documented to work**; the docs conflict | High for "built in the Nix sandbox" | Run the experiment in §3. If refused, use Cloud Run sandboxes or job-per-build (§4) |
| Nix's `sandbox-fallback = true` default would **silently** build unsandboxed | High (integrity) | Set `sandbox-fallback = false` in the builder's `nix.conf` |
| 60-minute request limit | Medium | Long builds go to jobs (168 h). Actions longer than 60 min stay local, or need an async execution path |
| No CPU outside requests (request-based billing) | Medium | Do all work, including uploads, inside the `Execute` stream. Run GC opportunistically at request start |
| `WaitExecution` / reconnect may land on another instance | Medium | Keep operation state in shared storage, or return `NOT_FOUND` and let clients retry `Execute` |
| Ephemeral disk is Preview, and its regional quota caps disk × max instances | Medium | Request a quota increase; use an in-memory volume for small stores |
| No published cold-start or job start-latency figures | Low to medium | Measure. Keep the executor image small (store paths are fetched per action) |
| Cloud Run functions cannot host a gRPC streaming server | Low: just don't use functions | Use services (and jobs) *(inferred: functions are request handlers behind a framework)* |
| 100 streams per HTTP/2 connection | Low | The client opens more channels |

## Contradictions with settled decisions

Checked against the map's Notes (turnkey-wit, "Settled while charting
(2026-09-26)").

1. **"Cloud Run (functions or services)".** Functions are a poor fit for the
   REAPI face *(inferred)*. The REAPI face needs a gRPC server with h2c and
   server streaming, which means a service. Event-driven functions also cap
   at 9 minutes ([functions comparison][cr-fn]). The backend is **services,
   plus jobs** for long Nix builds.
2. **"Executor results are shared across everyone … falling back … if Cloud
   Run can't isolate successive actions on one instance."** The facts
   **support keeping shared results**, but only through Cloud Run
   sandboxes, which are Preview. Plain instances cannot isolate successive
   requests. This is a conditional yes, not a contradiction: record the
   Preview dependency and the pending experiment in the decision.
3. **"Only the service's Nix builder produces store paths (built in the Nix
   sandbox, signed by the service key)".** Whether the **Nix sandbox** can
   run on Cloud Run is **unverified**, and the container contract leans
   against it. The trust rule survives. The phrase "built in the Nix
   sandbox" may have to become "built in an isolated Cloud Run sandbox or
   job task", which trades Nix's private store view for Cloud Run's
   isolation (§4).
4. **"Programmable base image … cached per instance"** holds, with a
   caveat. The per-instance cache is memory or ephemeral disk (Preview),
   and it dies on scale-in. It is a warm cache only.
5. **"Scale to zero is a hard requirement"** holds for services, jobs,
   in-memory volumes, ephemeral disks and GCS. It excludes NFS/Filestore
   for the store *(inferred)*.

## Not covered or not verified

- Whether gen2 lets a container create user, mount and PID namespaces.
  Needs the experiment in §3.
- What technology enforces Cloud Run sandboxes, and whether a detached
  process survives `sandbox do`.
- Cold-start time for gen2 with a small image, and job task start latency.
- Whether buck2 retries `Execute` on `NOT_FOUND` from `WaitExecution`.
- Filestore pricing (assumed provisioned, so never zero).
- Cloud Run "instances" (Preview, 2026-08-25): singleton, individually
  addressable instances ([release notes][cr-relnotes]). Not evaluated as an
  execution unit.

[cr-quotas]: https://docs.cloud.google.com/run/quotas
[cr-contract]: https://docs.cloud.google.com/run/docs/container-contract
[cr-envs]: https://docs.cloud.google.com/run/docs/about-execution-environments
[cr-envs-config]: https://docs.cloud.google.com/run/docs/configuring/execution-environments
[cr-security]: https://docs.cloud.google.com/run/docs/securing/security
[cr-grpc]: https://docs.cloud.google.com/run/docs/triggering/grpc
[cr-http2]: https://docs.cloud.google.com/run/docs/configuring/http2
[cr-timeout]: https://docs.cloud.google.com/run/docs/configuring/request-timeout
[cr-code-exec]: https://docs.cloud.google.com/run/docs/code-execution
[cr-sandboxes]: https://docs.cloud.google.com/run/docs/configuring/services/sandboxes
[cr-sandbox-blog]: https://cloud.google.com/blog/topics/developers-practitioners/google-cloud-run-sandboxes-are-in-public-preview/
[cr-relnotes]: https://docs.cloud.google.com/run/docs/release-notes
[cr-autoscaling]: https://docs.cloud.google.com/run/docs/about-instance-autoscaling
[cr-concurrency]: https://docs.cloud.google.com/run/docs/about-concurrency
[cr-billing]: https://docs.cloud.google.com/run/docs/configuring/billing-settings
[cr-maxinst]: https://docs.cloud.google.com/run/docs/configuring/max-instances
[cr-inmem]: https://docs.cloud.google.com/run/docs/configuring/services/in-memory-volume-mounts
[cr-ephdisk]: https://docs.cloud.google.com/run/docs/configuring/services/ephemeral-disk
[cr-gcsfuse]: https://docs.cloud.google.com/run/docs/configuring/services/cloud-storage-volume-mounts
[cr-nfs]: https://docs.cloud.google.com/run/docs/configuring/services/nfs-volume-mounts
[cr-fn]: https://docs.cloud.google.com/run/docs/functions/comparison
[cr-resmodel]: https://docs.cloud.google.com/run/docs/resource-model
[cr-tasktimeout]: https://docs.cloud.google.com/run/docs/configuring/task-timeout
[cr-tips]: https://docs.cloud.google.com/run/docs/tips/general
[cr-pricing]: https://cloud.google.com/run/pricing
[gcs-pricing]: https://cloud.google.com/storage/pricing
[moby-seccomp]: https://github.com/moby/profiles/blob/85e237f1fe229a0c61c9c7d8e743fa780d3b97ca/seccomp/default.json#L686-L780
[nix-clone]: https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/linux/build/linux-derivation-builder.cc#L575-L579
[nix-nouserns]: https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/linux/build/linux-derivation-builder.cc#L636-L641
[nix-userns-detect]: https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libutil/linux/linux-namespaces.cc#L14-L51
[nix-fallback]: https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/local-settings.hh#L418-L419
[nix-conf]: https://nix.dev/manual/nix/2.35/command-ref/conf-file.html

### Revisions read

- Cloud Run documentation pages: all showed "Last updated 2026-09-24 UTC",
  read on 2026-09-26.
- Cloud Run pricing and Cloud Storage pricing: read on 2026-09-26.
- `moby/profiles` at `85e237f1` (2026-09-25).
- Nix at tag 2.35.2 (commit `2c73b59d`); manual for Nix 2.35.2.
