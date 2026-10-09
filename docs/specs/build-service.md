# Spec: the build service

- **Status:** being charted. Sections are added as the map's tickets resolve; a missing section is still an open ticket.
- **Charted by:** the wayfinder map [Incremental builds everywhere for turnkey repos on GitHub](https://github.com/firefly-engineering/turnkey/issues/60). Each section cites the ticket that holds the full decision.
- **Vocabulary:** [`CONTEXT.md`](../../CONTEXT.md).

## Goal

Incremental builds actually running for turnkey repos on GitHub. The **build service** turnkey ships as a product: building blocks that any turnkey-managed repo on GitHub assembles to get incremental builds and tests on every machine (CI, laptops) without trusting PRs or developer machines. The backend is one service with two faces over shared storage, REAPI (content store, action cache, Execute with a Nix-programmable base image) and a Nix binary cache, deployed on Cloud Run and scaling to zero.

## Client: local execution hygiene

*Source: [Should tk own the buck2 daemon's environment?](https://github.com/firefly-engineering/turnkey/issues/68).*

**Any action can run in either place.** An action, Linux or darwin, can run locally or remotely wherever a compatible environment exists, as the same action with the same key. Byte-identical outputs are a goal, checked by [Verify action keys match across machines (G10)](https://github.com/firefly-engineering/turnkey/issues/55), not a guarantee: a divergent output costs downstream cache misses, never a wrong result. So a local action sees no environment except what it declares, exactly like a remote one. This holds even though local results are never shared: a leaked variable outside the key also makes the local daemon reuse outputs built under an older shell.

| Rule | Behaviour |
|---|---|
| **tk owns the daemon environment** | `tk` builds the environment buck2 runs with; it never forwards the dev shell's. |
| **Strict allowlist** | `HOME` and `TMPDIR` pinned to fixed per-repo locations, `LANG=C.UTF-8`, `TZ=UTC`, an empty or store-path-only `PATH`, and buck2's own `BUCK_*`/`BUCK2_*` settings. Nothing else. |
| **Tools declare what they need** | A variable a tool needs (`SDKROOT`, `MACOSX_DEPLOYMENT_TARGET`, …) is declared action env set by its toolchain, never inherited. Nix's `cc-wrapper` reading `NIX_CFLAGS_COMPILE`/`NIX_LDFLAGS` is the case that makes this necessary even with store-path toolchains. |
| **darwin SDK is a store path** | The macOS SDK comes from Nix (`apple-sdk`) and is declared as action env and inputs, so a future remote darwin executor sees the same action. The system Xcode SDK is never referenced. |
| **No secrets in the environment** | buck2 reads `$VAR` in `http_headers` from the daemon's environment, which would expose the token to every local action and pin it until a restart. The build service's credential is a client certificate file (`tls_client_cert`), which buck2 re-reads and Nix also accepts. |
| **Fingerprint and restart** | `tk` writes a fingerprint of the environment it built into the isolation directory. On any mismatch, including a missing fingerprint (a daemon started some other way), it kills and restarts the daemon, the path stale cells already take. |
| **Plain `buck2` goes through tk** | The dev shell shadows `buck2` with a wrapper that uses `tk`'s environment builder, so editors and scripts get the same daemon. The fingerprint is the safety net for anything that bypasses it. |
| **`tk run` keeps the user's environment** | The program `buck2 run` starts is the user's, not an action: buck2 execs it from the client process with the client's environment (`app/buck2_client/src/commands/run.rs` at the pinned revision, `envp: std::env::vars()`). Since `tk` runs the client with the scrubbed environment, `tk run` asks buck2 for the command (`--command-args-file`) and execs it itself with the user's environment. |
| **Unaffected** | `tk sync` runs in `tk`'s own process before buck2; the FUSE composition daemon is a separate process. Tests run in the daemon and get the scrubbed environment, as intended: a test that needs a variable declares it. |

**Ordering.** An empty `PATH` breaks every rule that finds `sh`, `python3` or a compiler on `PATH`, which is also exactly what breaks remote execution. Scrubbing lands after [Rust toolchain: give buck2 a store-path rustc instead of PATH (G1)](https://github.com/firefly-engineering/turnkey/issues/46), [Go toolchain: make GOROOT a keyed input instead of PATH (G2)](https://github.com/firefly-engineering/turnkey/issues/47) and [Turnkey-owned execution platform replacing prelude//platforms:default (G5)](https://github.com/firefly-engineering/turnkey/issues/50). Execution: [tk starts the buck2 daemon with a minimal pinned environment (G4)](https://github.com/firefly-engineering/turnkey/issues/49).

## Executor contract

*Source: [What does the executor need from an action to run it?](https://github.com/firefly-engineering/turnkey/issues/70). Key rule: [ADR 0014](../adr/0014-the-build-environment-stays-out-of-action-keys.md).*

**The Nix environment is transparent to buck2.** buck2 never asks Nix for anything and never knows it is there: whatever an action needs is already at the path it expects, on a laptop because the dev shell built it, on the executor because the service instantiated the **build environment** before running the action. This is why the REAPI and the Nix cache are one service: the environment an action runs in is provisioned from the same service's cache, cheaply, before any action starts.

| Rule | Behaviour |
|---|---|
| **Build environment** | A dedicated output of the repo's flake, generated by turnkey's flake module: the toolchains cell, the deps cells and the prelude, so everything buck2 actions can reach. Not the dev shell, which also carries editors and linters no action uses. |
| **Client evaluates** | The client evaluates the flake and submits the build environment's derivation closure through the realise API ([How can a client ask a remote service to build a derivation?](https://github.com/firefly-engineering/turnkey/issues/65)); the service answers with an **environment id**. This works alike for laptops with uncommitted changes, CI runners and the service-owned client host. Derivations are work, not results, so the trust rule holds. |
| **Session before buck2** | Before invoking buck2, `tk` has the service realise the build environment (one narinfo lookup per root when cached) and gets its id. A missing store path is the service's problem at session start, not a failed action. |
| **Environment id rides as a header** | `tk` writes the id into buck2's RE `http_headers`. It is not a platform property and not in any action key. When the environment changes, the daemon-environment fingerprint ([Client: local execution hygiene](#client-local-execution-hygiene)) restarts the daemon, so the header is never stale. |
| **Keys see store paths, not the environment** | An action reaches each tool through a reference its key covers: an absolute store path in argv, a declared `PATH` value, or a store link in its inputs. A store path stands for its binary and its whole closure, so a toolchain change re-keys only the actions that use it. An action that reaches a tool through an undeclared `PATH` has a dishonest key; [Verify action keys match across machines (G10)](https://github.com/firefly-engineering/turnkey/issues/55) is where that surfaces. |
| **Platform properties** | `turnkey-os` and `turnkey-arch` only, true facts the service routes on. No environment salt, no executor image version, no epoch. The properties are the same whether or not remote execution or a cache is configured, so turning the service on re-keys nothing. |
| **Sandbox sees the whole environment** | The action sees the build environment's closure read-only at `/nix/store`, as it sees the whole store locally, so one action behaves the same in both places. The executor does not discover store paths per action. |
| **Instance cache** | An executor instance that sees an environment id for the first time fetches its whole closure from the Nix cache onto ephemeral disk and keeps it for later actions. An unknown or unrealised id fails the action with a precondition error. |
| **Action environment** | The `Command`'s env, plus a fresh per-action `HOME` and `TMPDIR`, `LANG=C.UTF-8` and `TZ=UTC`; no `PATH` unless declared. The working directory is the input root. No network. |
| **One size** | One instance size, no sizing property (a property would enter every key). The 60-minute cap already sends oversized actions local. |

**Placement.** Remote is the default. `local_only` covers darwin actions (until a darwin executor exists), actions that may run past Cloud Run's 60-minute request cap, actions that need the network, actions that create namespaces themselves (bubblewrap, nested Nix builds, containers in tests: Cloud Run sandboxes are gVisor and refuse them, per [Do Cloud Run sandboxes isolate successive actions, and does the Nix sandbox run on Cloud Run?](https://github.com/firefly-engineering/turnkey/issues/76)), and anything a rule or target marks so. Racing local against remote is off. Fallback to local happens only on infrastructure errors (the service unreachable or timing out), never on an action failure, which would hide a contract violation. An action killed for exhausting the instance's memory or disk is an action failure, not an infrastructure error, although `sandbox do` reports it like one (exit 128, `WaitPID ... EOF`): the executor must classify it, or the client would rerun it locally.

**Supersedes** the per-action store-path discovery and the client's submit-and-retry from [How do remote-execution workers get Nix store paths on demand?](https://github.com/firefly-engineering/turnkey/issues/63); its signature-checking fetcher, per-instance cache and non-root sandbox uid still stand.
