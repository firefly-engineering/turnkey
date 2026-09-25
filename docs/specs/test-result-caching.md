# Spec: test result caching for `tk test`

- **Status:** ready for ticketing.
- **Charted by:** the wayfinder map `turnkey-w55`, "Test result caching for turnkey's buck2 tests". Each section cites the ticket that holds the full decision.
- **Mechanism:** [ADR 0001](../adr/0001-runner-recorded-native-test-caching.md).
- **Vocabulary:** [`CONTEXT.md`](../../CONTEXT.md): *test result caching*, *recorded result*, *result key*, *reuse policy*, *hit*, *cache-safe rule*.

## Goal

When a test's result key is unchanged, `tk test` reuses its recorded result and doesn't run the test again. This works on darwin and Linux, and the hit must survive:

- the same buck2 daemon;
- a daemon restart;
- `buck2 clean`;
- other checkouts of the same revision on the same machine (jj workspaces, git worktrees).

Sharing results across machines later has to need nothing more than pointing the same configuration at a standard remote-execution (REAPI) endpoint. Reusing results across platforms is not a goal.

## 1. Reuse policy

*Source: turnkey-w55.5.*

| Rule | Behaviour |
|---|---|
| **Only passes** | Only a passing run becomes a recorded result. A failure, timeout or fatal result always runs again. Nothing but passes is ever read or written, whether locally or, later, remotely. |
| **Default on** | Every target of a cache-safe rule (§4) is cached. |
| **Opting a target out** | The target label `no-test-cache` means "always run, never record". The reason goes in a comment next to it. There is only one label. |
| **Forcing a re-run** | `tk --rerun test <patterns>` is a tk-level flag, placed before the subcommand like `--no-sync`. It skips reads but still records fresh passes. There is no env-var or config equivalent. |
| **Result key** | Everything the test process can see: its inputs, argv (including `--test-arg` and the args from `.turnkey/local.toml`), declared env (including `--env`), timeout, working directory and platform. Plus a salt made of the buck2 version and the caching tool's version. Filters are argv, so a filtered run gets its own key. |
| **Correctness beats hit rate** | A wrong hit is never acceptable. When in doubt, miss. Anything a test reads from its surroundings is added to the key, cleaned out, pinned, or the test opts out. Hazards that need an unusual host state to trigger ("negligible") are accepted and noted (§4). |
| **Only under `tk`** | A plain `buck2 test` neither reads nor records. Only `tk` guarantees that the cells are fresh. |
| **Policy stays out of the key** | Changing the policy never splits the recorded results. |

## 2. What the user sees

*Source: turnkey-w55.6. Placement was settled in turnkey-w55.12.*

- **Per target:** a hit is marked `recorded` (`recorded, remote` once remote exists). The runner puts the marker in the per-test result it reports to buck2.
- **Summary:** hits are counted inside the pass count. For the `test` subcommand, `tk` runs buck2 as a child process and prints `N recorded` after it exits, using a count the runner leaves behind. Every other subcommand still hands the process over to buck2 (`exec`), as today.
- **Exit codes:** identical to a fresh run: 0 when everything passes, 32 when a test fails.
- **Output:** the recorded stdout and stderr are printed like a fresh pass's output. On demand, they are in the event log (`buck2 log show`). There is no separate log store and nothing is written to buck-out.
- **Structured output:** the event log is the only structured surface in v1. For each test result it shows whether it was a hit, whether the result came from a local or remote store, and the original duration. The console shows no duration for a hit. No new report format is added.

## 3. Mechanism

*Source: turnkey-w55.7 and [ADR 0001](../adr/0001-runner-recorded-native-test-caching.md). Verified in turnkey-w55.10, see [the feasibility research](../research/test-cache-mechanism-feasibility.md#verification-run-turnkey-w5510-2026-09-25).*

```
tk test ──probe──▶ bazel-remote (local, loopback TCP)
   │                     ▲  AC lookup (buck2, native)   ▲ ActionResult write (runner, passes only)
   └─child─▶ buck2 test ─┘                              │
                 └──▶ turnkey test runner (v2_test_executor) ─┘
```

- A cache-enabled test sets `supports_test_execution_caching = True` and a cache-reading `default_executor`, i.e. `CommandExecutorConfig(local_enabled = True, remote_enabled = False, remote_cache_enabled = True)`. The executor goes on the test provider, never on the execution platform, so build actions don't use the cache.
- buck2 computes the test's action digest, looks it up in the action cache, and on a miss runs the test locally. It reports the digest to the runner (`LocalCommand.action_digest`).
- After a **passing** local run, the runner writes an `ActionResult` under that exact digest. The entry has:
  - buck2's `instance_name`, the empty name: turnkey never sets `buck2_re_client.instance_name`, so the runner uses buck2's default rather than taking it as a flag;
  - stdout and stderr, inline or as CAS blobs;
  - `execution_metadata` with start and completion timestamps, which is where a hit's duration comes from;
  - output paths relative to the project root.
- buck2 serves hits natively (`RemoteCommand { cache_hit: true }`). The runner turns them into the `recorded` marker and the count.

Verified behaviour that the design depends on:

- **A recorded failure is replayed as a failure**, which is why only passes are written.
- **An unreachable server costs about 45 s of retries, then every opted-in test fails without running.** Skipping reads avoids this and still yields the same digest.
- **A daemon recovers once the server is back.**

## 4. Test rules

*Source: turnkey-w55.8. Hazard IDs refer to [test-undeclared-inputs.md](../research/test-undeclared-inputs.md).*

**Shared helper.** It lives in `nix/buck2/prelude-extensions/` (e.g. `test_caching.bzl`) and turns a rule's `ExternalRunnerTestInfo` arguments into their cache-enabled form. It sets:

- `supports_test_execution_caching = True`;
- the cache-reading `default_executor`;
- `use_project_relative_paths = True` and `run_from_project_root = True` (G1, P2);
- declared env `PATH`: Nix store paths only (bash, coreutils, plus what the toolchain needs);
- declared env `HOME=/homeless-shelter`. Together with the PATH setting this closes X1, S5, J2 and G3.

`go_test`, `rust_test` and `python_test` call the helper through small patches in `nix/patches/prelude/`. `solidity_test`, `jsonnet_test` and a future `typescript_test` call it directly. The helper is also the documented way for consumers' own test rules to opt in.

**The gate.** A rule is switched on for all its targets once its rule-level wrong-hit hazards are fixed. Spurious-miss fixes ship with it but don't block it. Hazards that belong to one target are fixed in that target, or the target gets the `no-test-cache` label.

| Language | Work before it's switched on | Order |
|---|---|---|
| Rust | Call the helper. | 1 |
| Jsonnet | Run jrsonnet against a `symlinked_dir` of the declared sources, and derive `-J` from it, not from `.` (J1). | 2 |
| Go | Helper patch. In turnkey's own repo, first fix G2 (bug turnkey-2gk: godeps fixtures not declared as resources) and G4 (use `os.Chtimes`). | 3 |
| Python | Give `system_python_toolchain` a store-path `interpreter` in `nix/buck2/mappings.nix` (P1, and P3 through it). Add `PYTHONDONTWRITEBYTECODE=1` to the declared env (P5). | 4 |
| Solidity | Generated `foundry.toml` passes the Nix `solc` and sets `offline = true` (S1). The soldeps remappings and sources become declared artifacts (S3). Fuzzing uses a fixed seed derived from the target label (S4). Stochastic fuzzing means labelling the target `no-test-cache`. | 5 |
| TypeScript | When `typescript_test` exists: call the helper and pass the same gate. | — |

Accepted as negligible: G5, P6 and R1.

## 5. The test runner

*Source: turnkey-w55.11, plus turnkey-w55.7 and turnkey-w55.12.*

- **A Rust crate in this repo** (a binary under `src/cmd/`). It replaces buck2's bundled OSS runner as `[test] v2_test_executor`.
- **Parity:** it reimplements the OSS runner's behaviour (`app/buck2_test_runner`): `--env`, `--timeout`, `--test-arg`, stdout and stderr in the result details, and exit code 32 on failure. A **parity suite** runs the same targets under both runners. It gates every buck2 version the runner supports.
- **Protocol code** is generated at build time, never hand-copied:
  - buck2's `test.proto` and `data.proto` come from buck2's source at the **same revision as the buck2 binary**. One Nix expression holds that revision, and both the binary and the protos read it.
  - The REAPI protos come from `bazelbuild/remote-apis` at a pinned revision.
- **REAPI calls:** it uses only FindMissingBlobs, BatchUpdateBlobs and UpdateActionResult.
- **Deterministic timeout:** the timeout it sends is part of the digest, so it is derived deterministically from the rule and the flags.
- **Mode flag:** the runner's mode comes from a flag after `--`: `on`, `record-only`, `read-only` or `off`. The runner consumes the flag, so it never reaches the test and never enters the result key. `tk` chooses the mode (§6); the runner obeys it and decides nothing about the reuse policy.
  - **Default is off.** It then sends `disable_test_execution_caching`, so buck2 never contacts the cache, and records nothing.
  - **record-only** skips reads and still records.
  - **read-only** reads and never records.
  - Only a target labelled `turnkey-cacheable` is recorded. The test-caching helper adds that label to every target of a cache-safe rule when caching is on, except those labelled `no-test-cache`: it returns their arguments unchanged, so they run on upstream's executor, which never reads a recorded result, and are never recorded. buck2 reports an action digest for every local run, cacheable or not, so without the label the runner would record passes nothing ever looks up.
- **Origin flag:** `tk` also passes whether the cache is `local` or `remote`, which the runner reports on each hit. The runner never infers it from the address.

## 6. `tk` and the local cache server

*Source: turnkey-w55.12.*

- **Server:** bazel-remote, pinned, referenced by store path, not on PATH.
- **Store:** one per user per machine, at `<platform cache dir>/turnkey/test-results/`. That is `~/Library/Caches/turnkey/…` on macOS and `~/.cache/turnkey/…` on Linux. It honours `TURNKEY_CACHE_DIR` and is shared by all checkouts and all turnkey repos.
- **Eviction:** bazel-remote's LRU only, default 5 GiB. The size is overridable per user through an env var or user-level config, never through the repo. There is no manual GC.
- **Address:** a fixed loopback TCP port chosen by turnkey, written into the generated `.buckconfig`. buck2's RE client can't use Unix sockets. The address is never passed with `-c`, because that would change the daemon's startup config. The port is overridable per user.
- **Lifecycle:** `tk` starts the server on demand, detached, the first time a cached `tk test` needs it. A lock in the cache directory prevents two servers. The server runs with `--idle_timeout` (e.g. 24 h), so an explicit stop command is optional. A launchd/systemd user service is a possible later add-on, not v1.
- **`tk test` sequence:**
  1. Check the cache can be used: probe the local server with a sub-second timeout, starting it if needed, or check that a remote endpoint accepts a connection within 2 s.
  2. Choose the mode (`testcache` applies it in `Config.RunTests`): for the local cache `on`, or `record-only` under `tk --rerun`; for a remote one `read-only`, or `off` under `tk --rerun`, since results are recorded only locally. An unusable cache → `off` and a one-line warning.
  3. Run buck2 as a child process.
  4. Print `N recorded`.
  5. Pass buck2's exit code through.

## 7. Shipping and configuration

*Source: turnkey-w55.9.*

- **Shipping:** the runner and bazel-remote come with the turnkey buck2 integration automatically, like `tk` and `deps-extract`. There is no `toolchain.toml` entry.
- **buck2 version:** turnkey keeps a table from buck2 version to buck2 source revision, and builds the runner for the consumer's selected `buck2-toolchain` version. An entry is added only after the parity suite passes against that version. If the consumer's buck2 version isn't in the table, the runner isn't configured, tests run uncached under buck2's own runner, and entering the shell prints a one-line notice.
- **Options:** devenv/flake-parts `buck2.testCache`:
  - `enable`, default `true`;
  - `endpoint`, default: the turnkey-managed local server.

  Per-machine settings (store size, port) never live in the repo.
- **Generated `.buckconfig`** (only when `enable` is true):
  - `[buck2_re_client]`: the engine, action-cache and CAS addresses all set to the endpoint, with `tls = false` for the local one;
  - `[test] v2_test_executor`: the runner's store path.
- **Remote:** setting `endpoint` to a remote REAPI address is the whole switch. `tk` stops managing a local server, only checks the endpoint accepts connections, and never asks the runner to record (see Out of scope).

## Out of scope

- Shared-cache handover: who may write to a shared cache, the trust model, uploading local passes, and a publish-only label.
- Choosing or deploying a shared REAPI backend.
- Caching build action results.
- Re-enabling tests in CI.
- Writing `typescript_test`.
- Caching per test case.
- Target determination and flaky-test management.

## Research behind this spec

- [buck2-pinned-test-caching.md](../research/buck2-pinned-test-caching.md): what the pinned buck2 supports (turnkey-w55.1).
- [local-re-api-servers.md](../research/local-re-api-servers.md): REAPI servers that can run on a developer machine (turnkey-w55.2).
- [test-undeclared-inputs.md](../research/test-undeclared-inputs.md): what each test reads beyond its declared inputs (turnkey-w55.3).
- [bazel-test-result-caching.md](../research/bazel-test-result-caching.md): Bazel's semantics (turnkey-w55.4).
- [test-cache-mechanism-feasibility.md](../research/test-cache-mechanism-feasibility.md): mechanism feasibility and the end-to-end verification (turnkey-w55.7, turnkey-w55.10).
