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
