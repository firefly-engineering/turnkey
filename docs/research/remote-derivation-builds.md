# How can an untrusted client ask a remote service to build a Nix derivation?

Research for ticket **turnkey-wit.5**, part of the wayfinder map **turnkey-wit**
("Incremental builds everywhere for turnkey repos on GitHub"). The question: how
can an *untrusted* client (a laptop, a CI runner, a fork PR) ask the build
service to build a Nix derivation, over a transport Cloud Run can serve
(HTTP/gRPC, no SSH daemon), so that the service's own Nix builder produces and
signs the output store paths? The map's trust rule, *clients submit work, never
results*, is taken as settled. This note checks what Nix already enforces for
it, what it does not enforce, and what that means for sharing signed outputs.

- **Researched:** 2026-09-26.
- **Nix:** release `2.35.2`, commit
  [`2c73b59d`](https://github.com/NixOS/nix/tree/2c73b59da29606068c0c98db015dd3a66955525d)
  (2026-08-12). Unless stated otherwise, every `NixOS/nix` link is a permalink
  at that commit. One fix only on `master` is cited at
  [`18057950`](https://github.com/NixOS/nix/tree/18057950ca430ae42decd96174af50fe5dba59e7).
  The local toolchain is Nix 2.34.7; nothing here depends on the difference.
- **Hydra:** [`NixOS/hydra@fef35cd4`](https://github.com/NixOS/hydra/tree/fef35cd48d10983a376831699b5c638e7c4d3975) (2026-09-26).
- **ofborg:** [`NixOS/ofborg@80074b96`](https://github.com/NixOS/ofborg/tree/80074b9649c2249c116a88650620f27a7f4030d7) (branch `released`, 2026-07-29).
- **Vendor docs** (nixbuild.net, garnix, FlakeHub, Hercules CI, Cloud Run) were
  read on 2026-09-26; the page's own date is given where it shows one.
- **Method:** source and manual reading, plus the GitHub security advisories
  of `NixOS/nix`. Nothing was built and no remote service was used.
  Statements not read off a source are marked *(inferred)*; statements that
  could not be checked are marked *(unverified)*.

Related notes: [remote-execution-and-caching.md](remote-execution-and-caching.md)
(the REAPI side, gaps G0–G11) and
[local-re-api-servers.md](local-re-api-servers.md).

## 1. TL;DR

1. **Nix already has the right trust split, but only over its own daemon
   protocol, and not over the network.** A client that is *not* in
   `trusted-users` can add `.drv` files and source paths (both
   content-addressed, so Nix checks them itself) and can ask the daemon to
   build a derivation that is already in the store. The daemon refuses the
   things that would let a client plant results. It refuses unsigned
   input-addressed store paths, "build this derivation I'm sending you inline"
   for input-addressed derivations, and client overrides of restricted
   settings such as `sandbox` and `substituters`
   ([§3.1](#31-what-the-daemon-lets-an-untrusted-client-do)). That is *clients
   submit work, never results* as implemented by Nix.
2. **Transport:** the worker protocol runs over a Unix socket, over
   `ssh`/`ssh-ng`, or over stdin/stdout (`nix daemon --stdio`). No store type
   speaks it over HTTP, gRPC or WebSocket. Nix's remote-builder feature (the
   build hook) is SSH-only, and the manual requires the SSH user to be in
   `trusted-users` ([§3.2](#32-remote-builders-the-build-hook)). So
   either we tunnel the daemon protocol through a byte stream Cloud Run
   accepts, or we define our own small API
   ([§6](#6-options)).
3. **Existing services don't offer an HTTP "build this" API for untrusted
   clients.** nixbuild.net is SSH plus per-account signing keys. Hydra's new
   `hydra-ad-hoc` serves the daemon protocol on a Unix socket, and its docs
   say members are "effectively a trusted Nix user". Hydra's builders talk to
   its queue runner over **gRPC**, which is the closest design to ours. garnix
   shares one cache across forks and PRs on the same argument as the map
   ([§4](#4-existing-services)).
4. **Recommendation:** a small **gRPC/HTTP "realise" API in front of a stock
   `nix-daemon`**, with the service acting as an *untrusted* client of its own
   daemon. The client uploads the `.drv` closure and the sources, both
   content-addressed and checked by the daemon on import. It then asks for
   realisation and streams the log back. The service signs and uploads the
   outputs *outside* the build host's reach
   ([§6.2](#62-option-b-a-realise-api-in-front-of-an-untrusted-daemon-connection-recommended),
   [§7](#7-minimal-api-sketch)). Tunnelling the raw daemon protocol (option A)
   is a fine prototype, but it hands clients the whole op surface, and the
   default `--stdio` mode *trusts* the peer.
5. **The trust caveat:** Nix itself can't guarantee that "an honest sandboxed
   build of an untrusted derivation can be shared". It holds only while the
   **build sandbox holds against a hostile derivation**. Nix's own advisories
   still give "do not allow untrusted builds on shared infrastructure" as the
   workaround. The latest critical escape
   ([CVE-2026-39860](https://github.com/NixOS/nix/security/advisories/GHSA-g3g9-5vj6-r3gj),
   April 2026) gave root to anyone who could submit a build. If a build
   escapes the sandbox, it can plant a bad output under an *input-addressed*
   path that a trusted build will later ask for, because nothing checks an
   input-addressed output against its content. The mitigations in
   [§5](#5-trust-analysis) keep the damage small but do not remove the risk.
   The settled decision stands as a **risk acceptance**, and the ADR
   (turnkey-wit.7) should say so.

## 2. What "submit work" means in Nix terms

- A **store derivation** (`.drv`) is a *text* store object: its store path is
  a hash of its content (the `text:` content-address method). An untrusted
  daemon client adds it with `AddTextToStore`, which computes the path itself
  ([`daemon.cc` L518-L537](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L518-L537)).
- When a `.drv` becomes valid in a local store, Nix re-derives its output paths
  and rejects the `.drv` if they don't match (`checkInvariants`,
  [`local-store.cc` L726-L738](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.cc#L726-L738);
  [`derivations.cc` L1388-L1409](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/derivations.cc#L1388-L1409)).
  An input-addressed output path is a hash of the derivation *modulo* its input
  derivations, so a `.drv` can only name output paths it actually hashes to.
  Computing that hash needs the input `.drv`s, so the **whole `.drv` closure**
  must be uploaded *(inferred from `hashDerivationModulo`'s reads of
  `inputDrvs`)*.
- **Sources** (`inputSrcs`: the flake source tree, patches, `builtins.path`
  imports) are content-addressed (`source`/NAR hash) store objects. They are
  accepted from untrusted clients because `checkSignatures` treats a correctly
  content-addressed path as fully trusted
  ([`path-info.cc` L108-L133](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/path-info.cc#L108-L133)),
  and `addToStore` rehashes the content to check the claim
  ([`local-store.cc` L1046-L1120](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.cc#L1046-L1120)).
- Everything else is a **result**: the outputs of input-addressed derivations.
  With `require-sigs = true` (the default), Nix imports one only if it carries
  a signature by a key in `trusted-public-keys`
  ([`globals.hh` L270-L287](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/globals.hh#L270-L287)).

So a client's "work" is exactly *the `.drv` closure plus the source paths*.
All of it is self-verifying, which is what the ticket assumed. There is one
edge case. A `.drv` may list an *input-addressed output* in `inputSrcs`. That
happens with `builtins.storePath` in impure evaluation, and the build hook
does it on purpose (see §3.2). Such a path is a result, and the service must
get it from its own cache or build it, never from the client.

## 3. Nix's own mechanisms

### 3.1 What the daemon lets an untrusted client do

The daemon marks each connection as trusted or not, and `performOp` branches
on that. At 2.35.2
([`daemon.cc`](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc)):

| Operation | Untrusted client | Source |
| --- | --- | --- |
| `AddTextToStore` (`.drv`), `AddToStore` (CA sources) | Allowed; the daemon computes the path from the content | [L405-L537](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L405-L537) |
| `AddToStoreNar`, `AddMultipleToStore` (arbitrary paths) | `dontCheckSigs` forced off: an input-addressed path needs a trusted signature, a CA path is verified by rehashing; `ultimate` forced false | [L491-L516](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L491-L516), [L914-L929](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L914-L929), [`local-store.cc` L1046-L1049](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/local-store.cc#L1046-L1049) |
| `BuildPaths` / `BuildPathsWithResults` on a `.drv` already in the store | Allowed, except `--repair` | [L539-L581](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L539-L581) |
| `BuildDerivation` (derivation sent inline, not in the store) | Refused for input-addressed derivations ("you are not privileged to build input-addressed derivations"); allowed for CA ones, with the `.drv` path recomputed | [L583-L653](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L583-L653) |
| `SetOptions` (client `--option`s) | Only `build-timeout`, `max-silent-time`, `poll-interval`, `connect-timeout` and an empty `builders` are honoured. `substituters` are limited to those already configured or in `trusted-substituters`. Everything else (e.g. `sandbox`, `require-sigs`, `allowed-impure-host-deps`) is ignored with a warning. Experimental features are never forwarded. | [L221-L298](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L221-L298) |
| `AddPermRoot`, `AddBuildLog`, repair (`VerifyStore`, `BuildPaths`) | Refused | [L678-L685](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L678-L685), [L888-L889](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L888-L889), [L1003-L1004](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L1003-L1004) |
| `RegisterDrvOutput` (CA-derivation realisations) | **Accepted without signature checks at 2.35.2** ([L976-L982](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L976-L982)). This is [GHSA-jm6c-h95p-6qhj](https://github.com/NixOS/nix/security/advisories/GHSA-jm6c-h95p-6qhj) (2026-08-05), fixed on `master` only ([L1028-L1034 at `18057950`](https://github.com/NixOS/nix/blob/18057950ca430ae42decd96174af50fe5dba59e7/src/libstore/daemon.cc#L1028-L1034)) and not backported. Only matters with `ca-derivations` enabled. | — |

What an untrusted client **cannot** do through settings:

- **`__noChroot`:** with `sandbox = true`, a derivation that sets `__noChroot`
  fails to build. On macOS, so does one with a custom sandbox profile. Only
  `sandbox = relaxed` lets them out
  ([`derivation-builder.cc` L1753-L1777](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/unix/build/derivation-builder.cc#L1753-L1777)).
  The client can't change `sandbox` (restricted setting, row above).
- **`__impure`:** impure derivations need the `impure-derivations`
  experimental feature on the daemon
  ([`experimental-features.cc` L46-L60](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libutil/experimental-features.cc#L46-L60)),
  and clients can't forward experimental features (row above).
- **Fixed-output derivations (FODs)** *do* get the network. On Linux, only
  sandboxed derivation types get a private network namespace
  (`CLONE_NEWNET` if `isSandboxed()`,
  [`linux-derivation-builder.cc` L575-L577](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/linux/build/linux-derivation-builder.cc#L575-L577)),
  and FODs are `sandboxed = false`
  ([`derivations.cc` L811](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/derivations.cc#L811)).
  The manual says the same
  ([`local-settings.hh` L375-L393](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/local-settings.hh#L375-L393)).
  Their output is hash-checked, so they can't plant a wrong result, but they
  can *reach whatever the build host can reach* (§5.3).
- **`allowed-uris` / `restrict-eval`** limit *evaluation*
  ([`eval-settings.hh` L183-L247](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libexpr/include/nix/expr/eval-settings.hh#L183-L247)).
  They don't apply here: clients evaluate, and the service only sees
  `.drv`s. They don't limit what an FOD's builder fetches at build time
  *(inferred: they are `EvalSettings`, not store settings)*.

The manual warns that "adding a user to `trusted-users` is essentially
equivalent to giving that user root access to the system"
([`nix/unix/daemon.cc` L59-L74](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/nix/unix/daemon.cc#L59-L74)).
So **clients must never be trusted users of the builder's daemon**.

### 3.2 Remote builders (the build hook)

- Supported transports are `ssh://` (legacy protocol) and `ssh-ng://`
  (daemon protocol through `ssh … nix-daemon --stdio`). The manual's
  requirements list an SSH server *and* "the username of the SSH user in the
  `trusted-users` setting"
  ([`distributed-builds.md` L12-L19](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/doc/manual/source/advanced-topics/distributed-builds.md#L12-L19)).
- Why trusted: the hook copies the derivation's already-built *inputs* to
  the remote with `NoCheckSigs`, then calls `BuildDerivation`
  ([`build-remote.cc` L303-L341](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/nix/build-remote/build-remote.cc#L303-L341)).
  It even overwrites `inputSrcs` with the input closure (L338). If the remote
  says the client is untrusted, the hook falls back to copying the `.drv`
  closure and calling `BuildPathsWithResults`
  (L354-L360). The untrusted remote then ignores `NoCheckSigs`, so unsigned
  inputs fail to import *(inferred from §3.1)*. Remote builders therefore
  work untrusted only if the remote can substitute every input from a cache
  it trusts. Our service can, because the inputs live in its own cache.
  Still, the hook is a push-results design, and SSH-only.

### 3.3 `--store` / `--eval-store`

`nix build --store ssh-ng://host --eval-store auto` evaluates locally, then
`RemoteStore::copyDrvsFromEvalStore` copies the `.drv` closure to the remote
and calls `buildPaths` there
([`remote-store.cc` L548-L604](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/remote-store.cc#L548-L604)).
This is exactly *submit work*: only `.drv`s and sources travel, and the
remote builds and substitutes itself. `nix copy --derivation` copies a
store derivation instead of its outputs
([`command.cc` L181-L190](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libcmd/command.cc#L181-L190)).
The outputs then sit in the *remote* store. The client has to fetch them
from the binary cache afterwards (`nix build --json` prints the output paths
to fetch). **This is the closest built-in match to the ticket.** Its only
problem is the transport.

### 3.4 Transport: can the worker protocol run over HTTP/gRPC/WebSocket?

- The daemon accepts connections on a Unix socket or on stdio.
  `nix daemon --stdio` "cannot see who is on the other side of a plain pipe"
  and so **trusts it by default**. `--force-untrusted` (experimental feature
  `daemon-trust-override`) makes it process ops itself with the client
  untrusted
  ([`nix/unix/daemon.cc` L443-L505, L575-L600](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/nix/unix/daemon.cc#L443-L600)).
- Client-side store types that speak the daemon protocol are `unix://`,
  `ssh-ng://` and `mounted-ssh-ng://`
  ([`uds-remote-store.hh` L54](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/uds-remote-store.hh#L54),
  [`ssh-store.hh` L38, L64](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/ssh-store.hh#L38)).
  `http(s)://` and `s3://` are binary-cache stores: they can hold paths but
  can't build. `ssh-ng` runs the `ssh` program from `PATH` with
  `NIX_SSHOPTS`
  ([`ssh.cc` L59, L188](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/ssh.cc#L188)).
- So the protocol is a plain byte stream, and in principle it can be
  tunnelled over any duplex channel. One way: a client-side relay on a local
  `unix://` socket (or a fake `ssh`) that forwards over a WebSocket or a
  gRPC bidi stream to a server-side `nix daemon --stdio --force-untrusted`
  *(inferred; not tried)*. Cloud Run carries both. WebSockets are supported,
  capped at the 60-minute request timeout, and not over end-to-end HTTP/2
  ([Cloud Run: WebSockets](https://docs.cloud.google.com/run/docs/triggering/websockets),
  updated 2026-09-24). All gRPC streaming types need end-to-end HTTP/2
  ([Cloud Run: gRPC](https://docs.cloud.google.com/run/docs/triggering/grpc),
  updated 2026-09-24). A long build that outlives 60 minutes needs a
  resumable job, not a single held connection.

### 3.5 Who signs

A daemon with `secret-key-files` signs every locally built output in-process
while registering it
([`globals.hh` L258-L268](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/globals.hh#L258-L268);
[`derivation-builder.cc` L1585, L1605](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/unix/build/derivation-builder.cc#L1605)).
That puts the **service's signing key in the memory of the root process that
also runs hostile builds**. Hydra keeps the key on the coordinator and signs
on upload (`store_uri …&secret-key=…`,
[`configuration.md` L77-L80](https://github.com/NixOS/hydra/blob/fef35cd48d10983a376831699b5c638e7c4d3975/subprojects/hydra-manual/src/configuration.md#L77-L80)).
That is the model to copy (§5.2).

## 4. Existing services

| Service | Transport for "build this" | Trust model / sharing | Source |
| --- | --- | --- | --- |
| **nixbuild.net** | SSH remote builder (`ssh://eu.nixbuild.net`); the HTTP API is read-only (`GET /builds`, `/usage`) | Every build in its own VM ("can access no other files than the build inputs"). A per-account key signs builds, a separate one signs uploads, and each account picks its `trusted-public-keys`. `store:write` permission covers uploading paths. Failures (not outputs) can be reused across accounts. Cross-account output sharing is not documented *(unverified)*. | [Getting started](https://docs.nixbuild.net/getting-started/), [settings](https://docs.nixbuild.net/settings/), [access control](https://docs.nixbuild.net/access-control/), [API](https://docs.nixbuild.net/api/) (undated) |
| **Hydra** | Server-side evaluation of trusted jobsets. Builders connect *out* to the queue runner over **gRPC** (`OpenTunnel` bidi stream, `BuildResult` streams `AddToStoreRequest`, presigned upload URLs). The optional, experimental `hydra-ad-hoc` serves the daemon protocol on a Unix socket. | Coordinator signs on upload. The ad-hoc socket "forwards uploads and build requests on a client's behalf without any checks of its own yet", so its group is "effectively a trusted Nix user" | [`architecture.md` L8-L64](https://github.com/NixOS/hydra/blob/fef35cd48d10983a376831699b5c638e7c4d3975/subprojects/hydra-manual/src/architecture.md#L8-L64), [`configuration.md` L155-L200](https://github.com/NixOS/hydra/blob/fef35cd48d10983a376831699b5c638e7c4d3975/subprojects/hydra-manual/src/configuration.md#L155-L200), [`streaming.proto` L14-L25](https://github.com/NixOS/hydra/blob/fef35cd48d10983a376831699b5c638e7c4d3975/subprojects/proto/v1/streaming.proto#L14-L25) |
| **garnix** | GitHub app; server-side eval and build of the flake | One signed cache (`cache.garnix.io`) shared across branches, PR origin/target, forks and unrelated repos. Argued safe because the key is "the hash of the definition of the package". The post doesn't discuss sandbox escapes. | [Nix, Caching, and CIs](https://garnix.io/blog/nix-caching-ci/) (2022-12-17), [caching docs](https://garnix.io/docs/caching/) |
| **ofborg** | GitHub bot; builds PRs on its own builders | Untrusted PR authors build on Linux. The "trusted users" list exists because some platforms lack "good sandboxing", and it is currently off only because "the current darwin builder is reset very frequently". It doesn't publish to a cache *(unverified: the README doesn't say)*. | [README L125-L140](https://github.com/NixOS/ofborg/blob/80074b9649c2249c116a88650620f27a7f4030d7/README.md#L125-L140) |
| **FlakeHub Cache** (Determinate) | No remote build offering found. Pushes come only from "trusted builders" (CI runners with OIDC JWTs); "no pushing from your laptop". Not available to fork PRs. | CI-signed uploads | [FlakeHub Cache docs](https://docs.determinate.systems/flakehub/cache/) (2025-11-10) |
| **Hercules CI** | Self-hosted agents that join a cluster with a token and build on the customer's machines | Customer holds the cache keys *(unverified)* | [Getting started](https://docs.hercules-ci.com/hercules-ci/getting-started/) |
| **Cachix Deploy** | Deploys already-built closures to agents; not a build service | — | *(not researched further)* |

What we learn from them: nobody exposes an HTTP "build this `.drv`" endpoint
to untrusted callers. The two services built for untrusted work
(nixbuild.net, ofborg) both lean on **per-build VMs or frequent host
resets**, not on the Nix sandbox alone. Hydra's builder protocol is the
nearest thing to a gRPC design we could borrow.

## 5. Trust analysis

### 5.1 The argument, and the assumption it rests on

The map's argument: the `.drv` fixes its output path (§2), and the service's
builder runs it honestly in the sandbox. So whatever lands at that path is
what *any* honest build of that `.drv` would give (up to nondeterminism), no
matter who asked. The same argument backs Nix's multi-user mode ("it is not
possible for one user to inject a Trojan horse into a package that might be
used by another user",
[`introduction.md` L55-L63](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/doc/manual/source/introduction.md#L55-L63))
and garnix's shared cache (§4).

What it assumes is that **the builder process is not compromised by the
build**. For *content-addressed* outputs (FODs, floating CA) the content is
checked afterwards, and the assumption barely matters. For *input-addressed*
outputs nothing checks the content afterwards. The daemon's own comment says
"that the output data actually came from those derivations is fundamentally
unverifiable, but the daemon trusts itself on that matter"
([`daemon.cc` L605-L610](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/daemon.cc#L605-L610)).

**The attack:** an attacker submits a `.drv` that a trusted build will
request later, such as the `godeps-cell` of a dependency bump that main is
about to land. They make it escape the sandbox and write a bad output. The
service signs it, and main substitutes it. Knowing the future `.drv` exactly
is easy: the PR *is* the future main, and `.drv`s are deterministic from the
source. Nothing in Nix detects this.

### 5.2 How often the sandbox gives way

From the `NixOS/nix` security advisories (fetched 2026-09-26):

- [CVE-2026-39860 / GHSA-g3g9-5vj6-r3gj](https://github.com/NixOS/nix/security/advisories/GHSA-g3g9-5vj6-r3gj),
  *critical*, 2026-04-07. A symlink trick during FOD output registration let
  any user allowed to submit builds overwrite root-writable files: "gain root
  privileges". Fixed in 2.28.6 … 2.34.5. The workaround given is "Do not
  allow untrusted builds to be submitted to the Nix daemon."
- [CVE-2024-27297 / GHSA-2ffj-w4mj-pg37](https://github.com/NixOS/nix/security/advisories/GHSA-2ffj-w4mj-pg37),
  2024-03. FODs could pass file descriptors via abstract Unix sockets and
  modify a *registered* output. The workaround given is "Do not allow
  untrusted builds to occur on shared infrastructure." The 2026 fix adds
  landlock hardening, but only on kernels ≥ 6.12 with landlock enabled.
- [CVE-2024-38531](https://github.com/NixOS/nix/security/advisories/GHSA-q82p-44mg-mgh5)
  (sandbox escape),
  [CVE-2024-45593](https://github.com/NixOS/nix/security/advisories/GHSA-h4vv-h3jq-v493)
  (unsafe NAR unpacking, critical),
  [CVE-2026-44029](https://github.com/NixOS/nix/security/advisories/GHSA-gr92-w2r5-qw5p)
  and [GHSA-vh5x-56v6-4368](https://github.com/NixOS/nix/security/advisories/GHSA-vh5x-56v6-4368)
  (archive/NAR parsing: input the service would parse from clients).
- macOS:
  [CVE-2024-51481](https://github.com/NixOS/nix/security/advisories/GHSA-wf4c-57rh-9pjg)
  (sandbox escape via built-in builders) and
  [CVE-2025-53819](https://github.com/NixOS/nix/security/advisories/GHSA-qc7j-jgf3-qmhg)
  (2.30.0 ran builds as root).

So "the sandbox holds" is **a real, recurring risk rather than an
invariant**. Upstream's standing advice for the builder-compromise class is
not to take untrusted builds at all.

### 5.3 What keeps the damage small (defence in depth)

None of these is new policy. They are the conditions under which the settled
decision is defensible.

1. **Key off the build host.** Don't set `secret-key-files` on the builder
   daemon (§3.5). A separate signer, outside the build host's reach, signs
   the outputs *of the build it dispatched* when it uploads them (Hydra
   model). A compromised build then can't steal the key or sign arbitrary
   paths. It can still poison the outputs of the `.drv` it submitted, which
   is the §5.1 attack.
2. **One build per fresh, disposable sandbox host.** Use a fresh instance or
   microVM per build (nixbuild.net). Never reuse a builder's local store
   across requesters, and treat an instance as tainted after any untrusted
   build. That way an escape can't tamper with *other* builds or with store
   paths later uploaded from the same host. On Cloud Run this depends on
   instance reuse, which is turnkey-wit.1's question. On Cloud Run gen1
   (gVisor, "emulation of most, but not all operating system calls") the
   Nix sandbox may not work at all. Gen2 is a microVM with "all system
   calls, namespaces, and cgroups"
   ([execution environments](https://docs.cloud.google.com/run/docs/about-execution-environments),
   updated 2026-09-24).
3. **`sandbox = true` and `sandbox-fallback = false`.** `sandbox-fallback`
   defaults to **true**: on a kernel without mount/PID namespaces, Nix
   *silently builds unsandboxed*
   ([`local-settings.hh` L418-L419](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/local-settings.hh#L418-L419),
   [`derivation-builder.cc` L1789-L1795](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/unix/build/derivation-builder.cc#L1789-L1795)).
   In a container this is the likeliest way to fail open.
4. **Nothing worth stealing on the network.** FODs share the host's network
   (§3.1). On Cloud Run the metadata server at `metadata.google.internal`
   hands out access tokens for the service identity
   ([container contract](https://docs.cloud.google.com/run/docs/container-contract),
   updated 2026-09-24). A hostile FOD can therefore act as the builder's
   service account. The builder's identity must hold **no write access** to
   the cache bucket, the signer or anything else. Egress should go through a
   proxy that denies internal ranges if the platform allows it *(unverified
   for Cloud Run)*. Don't put a `netrc` on builders
   ([CVE-2024-47174](https://github.com/NixOS/nix/security/advisories/GHSA-6fjr-mq49-mm2c)).
5. **No experimental features that widen the attack surface** on the
   builder: no `ca-derivations` (GHSA-jm6c, §3.1), no `impure-derivations`,
   no `recursive-nix`
   ([CVE-2026-64846](https://github.com/NixOS/nix/security/advisories/GHSA-6h4g-g5j9-fm5f)).
6. **Patch fast.** Pin a Nix whose advisory list is clean, and treat a new
   sandbox advisory as a trigger to rebuild or evict input-addressed outputs
   built from untrusted requests since the vulnerable version shipped
   *(inferred; needs provenance per path: who asked, which builder
   version)*.
7. **Optional, stronger: never serve untrusted-built input-addressed outputs
   to trusted consumers.** Rebuild them from a trusted request, or check
   them with a second independent build (`--check`-style). This is the
   only mitigation that *removes* the §5.1 attack rather than shrinking it.
   It means the Nix cache gets two tiers, which contradicts the settled
   "the Nix cache therefore needs no scopes". Flagged, not proposed.

### 5.4 FODs and client-claimed sources

- **FODs** are safe to share *as results*: the output must match the
  declared hash. Their risks are (a) what they can reach (§5.3 item 4), and
  (b) cross-FOD collusion and post-registration tampering (CVE-2024-27297
  class). Turnkey's dependency cells are mostly FODs (one fetch per module,
  hash from the lock file:
  [`nix/lib/deps-cell/adapters/go.nix` L91-L107](../../nix/lib/deps-cell/adapters/go.nix#L91-L107))
  plus an input-addressed `runCommand` that assembles the cell
  ([`nix/lib/deps-cell/default.nix` L95](../../nix/lib/deps-cell/default.nix#L95)).
  That final assembly step is where §5.1 applies.
- **Sources the client uploads** are content-addressed and rehashed on
  import (§2), so a client can't pass off different content under a source
  path. A client *can* upload any content it likes as a new source. That is
  just "work".
- **Input-addressed paths the client claims as inputs** (in `inputSrcs`, or
  wanted by the build hook) must be refused on upload. The stock daemon
  already does this for untrusted clients unless a trusted key signed them
  (§3.1). The service must then substitute them from its own cache or build
  them.

### 5.5 Darwin (later)

On macOS the sandbox is off by default
([`local-settings.hh` L393](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/include/nix/store/local-settings.hh#L393)),
is `sandbox-exec`-based rather than namespace-based, and has its own escape
history (CVE-2024-51481, CVE-2025-53819). ofborg treats darwin as a platform
"without good sandboxing" and relies on frequent resets (§4). A darwin
builder that shares outputs of untrusted derivations needs item 2 of §5.3
(fresh VM per build, e.g. a macOS VM image) even more than Linux does.
Otherwise darwin builds from untrusted requests should not be shared
*(inferred)*.

## 6. Options

### 6.1 Option A: tunnel the daemon protocol

A client-side relay exposes `unix:///…` (or poses as `ssh` for `ssh-ng://`)
and forwards to the service over a WebSocket or gRPC bidi stream. The
server end is `nix daemon --stdio --force-untrusted`, one process per
connection. Clients then just run
`nix build --store unix://… --eval-store auto`, and `copyDrvsFromEvalStore`
does the upload (§3.3).

- **For:** zero protocol design, full Nix UX (logs, `--json`), and untrusted
  handling is upstream's.
- **Against:** it exposes the *whole* daemon op surface (NAR parsing,
  `AddToStoreNar`, queries, `AddSignatures`, `RegisterDrvOutput` …) to the
  internet. Forgetting `--force-untrusted` means full trust. The 60-minute
  connection cap applies (§3.4). There is no natural spot to put the signer
  "outside" the build host. Each Cloud Run request needs a daemon *and*
  its store to live for the whole build. Hydra itself calls its equivalent
  experimental and access-control-free (§4).

### 6.2 Option B: a realise API in front of an untrusted daemon connection (recommended)

The service speaks a narrow gRPC/HTTP API to clients. Internally it is an
**untrusted** client of a stock `nix-daemon` on a disposable builder, so the
daemon's untrusted rules (§3.1) are a second line of defence behind the
API's own checks. The signer/uploader sits outside the builder, and the
builder's identity can't write the cache. This matches Hydra's split (a
coordinator that signs, builders that stream results back) and the REAPI
face's "only the executor writes results".

### 6.3 Option C: evaluate on the service

The client sends a flake ref and commit, and the service evaluates
(`restrict-eval`, `allowed-uris` now apply). No `.drv` upload is needed, but
the service pays for evaluation and must fetch the source. It also doesn't
cover laptops with uncommitted changes. This fits the fork-PR "service-owned
client host", not the general case.

## 7. Minimal API sketch

```text
PutObjects(stream)   # .drv files (text CA) and sources (NAR, CA method + hash);
                     # server imports each via AddTextToStore / AddToStore,
                     # i.e. recomputes the path; anything else is rejected.
                     # Objects already present (QueryValidPaths) are skipped.
Realise(drv_path, outputs) -> job_id
                     # server: .drv present + valid (checkInvariants passed on import),
                     # closure complete, platform/features acceptable, policy checks;
                     # dedupe on drv_path: an in-flight or finished job is reused.
Watch(job_id) -> stream {log line | status | result{output name -> store path}}
                     # resumable; survives the 60-min request cap by reconnecting.
# Outputs are fetched from the Nix binary cache face, signed by the service.
```

Checks the service must make before building (beyond what the daemon does):

1. Every uploaded object is **content-addressed**, and the server derives
   its path from the content (§2). Never accept a client-supplied
   `ValidPathInfo` for input-addressed paths.
2. The `.drv` closure is complete and each `.drv` passes `checkInvariants`,
   which the stock daemon does on import.
3. **Derivation policy on the parsed `.drv`:** `system` is one the builder
   serves. `requiredSystemFeatures` is within an allow-list (e.g. no `kvm`
   unless the builder is a VM). No `__impure`, and no `__noChroot` (already
   fatal with `sandbox = true`). Optionally, a cap on the number of FODs and
   an allow-list of FOD URL hosts *(inferred policy; Nix has no build-time
   URL allow-list)*.
4. `inputSrcs` that are not content-addressed must already be in the
   service's cache, never uploaded (§5.4).
5. **Builder configuration:** `sandbox = true`, `sandbox-fallback = false`,
   `require-sigs = true`, `trusted-users = root` only, substituters =
   the service's own cache (+ upstream caches it chooses), no
   `secret-key-files`, and no experimental features beyond `nix-command`.
6. **Signing:** the signer takes the output paths *of the job it
   dispatched*, reads them from the (now idle) builder, signs, and uploads.
   It records provenance (requester, `.drv`, builder image/Nix version) so
   §5.3 item 6 is possible.
7. Resource limits (`build-timeout`, `max-silent-time`, output size) and
   quota per requester. That is cost, which belongs to "quotas and abuse
   controls".

## 8. Recommendation

- Build **Option B**: a narrow realise API in front of a disposable builder
  whose `nix-daemon` sees the service as an untrusted client. Signing and
  upload happen in a component the builder can't reach, and the builder's
  cloud identity has no write rights.
- Use Option A only as a prototype to measure the real flow (`nix build
  --store … --eval-store auto`) before writing the API.
- Carry §5.3 items 1–5 into the spec as **requirements** of the Nix builder
  block (inputs for turnkey-wit.10 and turnkey-wit.12). Item 2 depends on
  turnkey-wit.1's answer about per-build instance isolation on Cloud Run.
- Have the trust-rule ADR (turnkey-wit.7) state plainly that sharing
  input-addressed outputs of untrusted derivations is a **risk accepted on
  the strength of the Nix sandbox plus VM isolation**, with a response plan
  for sandbox advisories (item 6). Mention item 7 (trusted rebuild or
  second build before trusted consumers use an output) as the escalation if
  that risk is ever judged too high.

## 9. Open questions

- Does Cloud Run gen2 let `nix-daemon` create the namespaces the sandbox
  needs (user namespaces, `CLONE_NEWNET`, mount), and does it run each
  request on a fresh instance? (turnkey-wit.1)
- Can Cloud Run egress be restricted so FODs can reach the internet but not
  the metadata server? *(unverified)*
- How much of a real PR's `.drv` closure is already present? That decides
  whether upload cost matters (`QueryValidPaths` dedup makes it incremental).
- Should outputs built from untrusted requests carry a provenance marker in
  the cache (e.g. a narinfo field or a side table) so that an advisory can
  evict them?
