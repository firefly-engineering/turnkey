# Test result caching in the pinned buck2

Research for ticket `turnkey-w55.1`: *What test-caching support does our pinned buck2 have?*

- **Researched:** 2026-09-25.
- **Sources:** primary sources only. These are the buck2 source at the pinned commit, the upstream commit history, the release assets on GitHub, the `toolbox` flake at the revision in `flake.lock`, and quokka's repository.
- **Method:** nothing was built or run. The buck2 binary was only inspected with `strings`.

**Pinned buck2:** release tag [`2026-04-15`](https://github.com/facebook/buck2/releases/tag/2026-04-15), which is commit
[`7600cb80070a88b88be67aa5d20d6a93cffa0223`](https://github.com/facebook/buck2/commit/7600cb80070a88b88be67aa5d20d6a93cffa0223)
(committed 2026-04-14). Unless stated otherwise, every buck2 link below is a permalink at that commit.

Upstream's name for the feature is *test execution caching* (for example `supports_test_execution_caching`). This document says
*test result caching*, meaning reusing a recorded test result instead of re-running the test. It uses upstream identifiers only where it quotes code.

## Summary

| # | Question | Answer at `7600cb80` | Key source |
|---|---|---|---|
| 1a | Does `ExternalRunnerTestInfo` accept `supports_test_execution_caching`? | **Yes.** It is an optional bool and defaults to `False`. Only the test *stage* reads it, and only as permission to *read* the remote action cache. | [`external_runner_test_info.rs` L113-L114, L203-L209, L577](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L113-L114) |
| 1b | Are stable test output paths the default? | **Yes.** No knob remains. Test outputs go to `…/test/execution/<target>/<hash(variant, repeat_count, testcases)>`. | [`orchestrator.rs` L2057-L2089](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L2057-L2089), [91c4d7d109](https://github.com/facebook/buck2/commit/91c4d7d1097451b2e434de1ad4a8880c7bf7604f) |
| 2a | With local execution on, remote off and remote cache on, do tests get cache lookups? | **Yes, if three conditions hold.** The test must opt in, the runner must not send `disable_test_execution_caching`, and `--no-remote-cache` must be unset. The lookup goes to the **RE action cache**. With the OSS default (`remote_cache_enabled` unset) there is no lookup at all. | [`orchestrator.rs` L1248-L1315, L1109-L1138](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1248-L1315), [`command_executor_config.rs` L308-L372](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L308-L372) |
| 2b | Are local test executions ever uploaded? | **No, never.** The test stage always uses `NoOpCacheUploader` (the code comment reads "We never upload local test executions"). Only tests that ran on RE can produce hits. Test *listings* can be uploaded. | [`orchestrator.rs` L1302-L1315](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1302-L1315), [d1d736ed45](https://github.com/facebook/buck2/commit/d1d736ed45a661aa86e2000a1d598cf6bde915e9) |
| 3a | How does `allow_cache_uploads` behave for `run` actions? | **Two gates, both required.** The executor needs `allow_cache_uploads=True` with remote cache on. The action needs `allow_cache_upload=True`, or else `buck2.default_allow_cache_upload=true`. Only successful, locally executed results are written to the action cache. | [`common.rs` L426-L452](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L426-L452), [`run.rs` L1549-L1588](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1549-L1588), [`caching.rs` L576](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L576) |
| 3b | Local `LocalActionCache` / dep-file reuse | **Yes, in memory only.** A static map per daemon holds identical-action and dep-file matches. It is checked before the RE cache and lost when the daemon restarts. Tests never use it. | [`dep_files.rs` L94-L95, L474-L547, L1053-L1128](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L94-L95) |
| 3c | Does `buck2.sqlite_dep_file_state` exist? | **No.** The database landed in [2ca2802708](https://github.com/facebook/buck2/commit/2ca2802708b96e064fbb27be7a2f2685cbef22a4) (2026-07-31) and the config key in [9944684e95](https://github.com/facebook/buck2/commit/9944684e952be7de56507ec1dd3759e9759220a6) (2026-08-11). The first release with the key is `2026-08-22`. | `git grep` at the pin (no matches) |
| 4 | Is `test.proto` identical to quokka's vendored copy? | **Yes, byte for byte** (git blob `18ca8a58…`). `host_sharing.proto` and `downward_api.proto` are also identical. quokka's `data.proto` is a deliberate hand-written subset, and its fields agree with the pin. | §4 |
| 5 | How does toolbox produce buck2? | **Prebuilt upstream release binary.** It `fetchurl`s `buck2-<triple>.zst` and decompresses it, with no source build, so `patches` can't apply. A patch needs a from-source derivation: nightly-2026-01-18 and a self-generated `Cargo.lock`. A bump needs one `data.json` entry plus a new `buck2-toolchain` version. | [toolbox `packages/buck2/default.nix` L15-L54](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2/default.nix#L15-L54) |

## 0. How the pin was identified

1. `buck2-toolchain = { version = "3" }` in [`toolchain.toml` L6](../../toolchain.toml#L6).
2. The `toolbox` input is locked at `9b190de95aa36df3215cfc239c44668f5ec47109` ([`flake.lock` L747-L776](../../flake.lock#L747-L776)).
3. In toolbox, `buck2-toolchain` version `"3"` is `buck2 = "2026-04-15"` plus `reindeer = "2026.04.27.00"`
   ([`packages/buck2-toolchain/data.json` L2-L6](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2-toolchain/data.json#L2-L6)).
   The `buck2` package's default, and its only 2026-04 entry, is `2026-04-15`
   ([`packages/buck2/data.json` L2-L3](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2/data.json#L2-L3)).
4. The GitHub tag `refs/tags/2026-04-15` points at commit `7600cb80070a88b88be67aa5d20d6a93cffa0223`. The release was published 2026-04-15T01:18:03Z
   (`gh api repos/facebook/buck2/git/ref/tags/2026-04-15`).
5. The store binary `/nix/store/hzn7gypz…-buck2-2026-04-15/bin/buck2` (aarch64-darwin) embeds
   `2026-04-14-7600cb80070a88b88be67aa5d20d6a93cffa0223`. That matches the upstream `<date>-<sha>` version scheme stamped through
   `BUCK2_SET_EXPLICIT_VERSION`
   ([`upload_buck2.yml` L17-L21, L121-L124](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/.github/workflows/upload_buck2.yml#L17-L21)).

## 1. `ExternalRunnerTestInfo` and stable test output paths

### 1a. `supports_test_execution_caching`

- **Field.** The provider field is documented as "Whether test execution results can be read from the remote action cache"
  ([L113-L114](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L113-L114)).
  It is a named constructor parameter defaulting to `None`
  ([L577](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L577)),
  is validated as a bool
  ([L541-L544](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L541-L544)),
  and reads as `false` when unset
  ([L203-L209](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L203-L209)).
- **Effective setting.** The orchestrator computes
  `supports_test_execution_caching() && !disable_test_execution_caching`
  ([`orchestrator.rs` L398-L399](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L398-L399)).
  The second operand is `ExecuteRequest2.disable_test_execution_caching = 11`, which the test runner sets per request
  ([`test.proto` L187](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L187)).
  buck2's bundled OSS runner always sends `false`
  ([`buck2_test_runner/src/runner.rs` L190-L202](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_runner/src/runner.rs#L190-L202)).
- **Prelude support.** Only Java, Android and Kotlin test rules expose the attribute, for example
  [`prelude/java/java_test.bzl` L198](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/prelude/java/java_test.bzl#L198).
  The same holds in the prelude turnkey actually uses, `buck2-prelude@27c8628d`
  ([`java/java_test.bzl` L191](https://github.com/facebook/buck2-prelude/blob/27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795/java/java_test.bzl#L191)).
  No Rust, Go, Python or C++ test rule sets it. A wrapper rule has to re-emit the provider with the flag set, which is what quokka's
  [`rules/cached_rust_test.bzl`](https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/rules/cached_rust_test.bzl) does.
- **History.** All of these commits are ancestors of the pin:
  - [d1d736ed45](https://github.com/facebook/buck2/commit/d1d736ed45a661aa86e2000a1d598cf6bde915e9) (2026-02-26) added the opt-in. Its message says: *"We don't cache on DICE, and we don't upload local executions, so at this point we only ever get cache hits for tests that were run on RE."*
  - [6bc7e7bde4](https://github.com/facebook/buck2/commit/6bc7e7bde432d6714d58fe1e982e2f5166f983c2) disables test result caching for stress runs.
  - [f8b2b1f44d](https://github.com/facebook/buck2/commit/f8b2b1f44dc1eb8c9263650077297411e2efb0ab) added the `--disable-test-execution-caching` flag to TPX. TPX is Meta's internal runner, not the OSS one.

### 1b. Stable (deterministic) test output paths are the default

- **At the pin.** `resolve_output_root` sends listings to `resolve_test_discovery(target)`. Test runs go to
  `resolve_test_execution(target, hex(hash(variant, repeat_count, testcases)))`
  ([`orchestrator.rs` L2057-L2089](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L2057-L2089)),
  which resolves under `test/execution`
  ([`buck_out_path.rs` L387-L398](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_core/src/fs/buck_out_path.rs#L387-L398)).
  `orchestrator.rs` at the pin no longer contains `use_deterministic_test_execution_paths`, `Uuid` or `TestExecutionPrefix`.
- **History.**
  - [d2b7ecb171](https://github.com/facebook/buck2/commit/d2b7ecb1716144a9326fbd44395e9a445d631251) (2026-02-23) replaced the per-session timestamp and UUID prefix with a target-based path. It was **backed out** the next day in [47a8fda2d4](https://github.com/facebook/buck2/commit/47a8fda2d463db9ddbc3a9f763a78cbb857867da).
  - It re-landed behind a buckconfig flag in [c82ee98501](https://github.com/facebook/buck2/commit/c82ee98501d01e81a37cc94b79100cd52582b7b4) (2026-02-26).
  - It became unconditional in [91c4d7d109](https://github.com/facebook/buck2/commit/91c4d7d1097451b2e434de1ad4a8880c7bf7604f) (2026-03-17), which removed `use_deterministic_test_execution_paths`. [39c11602ef](https://github.com/facebook/buck2/commit/39c11602ef95f3a7f50c0e7d2213e25495a5bc76) removed `TestExecutionPrefix` the same day.
- **Upstream tests.** The e2e test `test_stable_action_digest_with_deterministic_paths` asserts that two runs produce identical test action digests. `test_stress_runs_have_different_action_digests` asserts that stress repeats differ, because `repeat_count` is in the hash
  ([`tests/core/test/test_execution.py` L16-L70](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/tests/core/test/test_execution.py#L16-L70)).
- **Caveat.** The path hash uses `BuckDefaultHasher`, which is `std::collections::hash_map::DefaultHasher`
  ([`shed/buck2_hash/src/lib.rs` L237](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/shed/buck2_hash/src/lib.rs#L237)).
  That output is stable run to run for a given binary. std, however, does not promise the algorithm across Rust releases. A buck2 bump could therefore
  change test action digests. The effect would be cache misses, not wrong hits.

## 2. Test result cache lookups and uploads (local on, remote off, remote cache on)

Below is the path taken by `CommandExecutorConfig(local_enabled = True, remote_enabled = False, remote_cache_enabled = True)` at the pin.

1. **Starlark to Rust config.** In OSS builds `remote_cache_enabled` defaults to `remote_enabled`, so it must be passed explicitly
   ([`command_executor_config.rs` L308-L318](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L308-L318)).
   `(Some(local), None, true)` becomes `Executor::RemoteEnabled { executor: RemoteEnabledExecutor::Local(local), remote_cache_enabled: true, … }`,
   and RE properties and use case get defaults
   ([L352-L370](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L352-L370)).
   With the cache off, the result is a plain `Executor::Local`
   ([L372](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L372)).
2. **Per-test override.** `executor_config_with_remote_cache_override` leaves the config unchanged for listings, and also for tests that support caching.
   For every other test it turns `remote_cache_enabled` off
   ([`orchestrator.rs` L1248-L1284](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1248-L1284)).
3. **Executor factory** (`app/buck2_server/src/daemon/common.rs`):
   - `Executor::Local` gets a no-op cache checker and a no-op uploader
     ([L260-L273](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L260-L273)).
   - For `RemoteEnabled`, caching is disabled by `BUCK2_TEST_DISABLE_CACHING`, by `skip_cache_read`, or when both the remote cache and the remote dep-file cache are off
     ([L278-L284](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L278-L284)).
     `skip_cache_read` is `--no-remote-cache`
     ([`build.rs` L146, L272](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client_ctx/src/common/build.rs#L272)).
   - Otherwise it builds an `ActionCacheChecker`
     ([L320-L336](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L320-L336)),
     which calls `re_client.action_cache(…)`
     ([`action_cache.rs` L107](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L107)).
     This needs `[buck2_re_client]` addresses, `action_cache_address` among them
     ([`buck2_re_configuration/src/lib.rs` L238](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_re_configuration/src/lib.rs#L238)).
     `RemoteEnabledExecutor::Local` still runs commands on the local executor
     ([L343-L345](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L343-L345)).
4. **Test stage wiring.** For the `Testing` stage the orchestrator **always** replaces the uploader with `NoOpCacheUploader`, under the comment "We never upload local test executions".
   It keeps the action cache checker only when caching is effective
   ([`orchestrator.rs` L1302-L1315](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1302-L1315)).
5. **Execution.** In the `Testing` arm of `execute_request`, an opted-in test calls `executor.action_cache(…)` first. A `Break` is a hit; a `Continue` falls through to `exec_cmd`
   ([L1109-L1155](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1109-L1155)).
   That arm never calls `cache_upload`.
   The comment at [L1026](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1026)
   ("For test execution, we currently do not do any cache queries") is stale.
6. **Hit reporting.** The runner sees a hit as `ExecutionDetails.execution_kind = RemoteCommand { cache_hit: true, cache_hit_type: ACTION_CACHE }`
   ([`kind.rs` L158-L167](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L158-L167);
   [`orchestrator.rs` L370-L372](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L370-L372)).
7. **No in-daemon reuse.** DICE memoises only `cacheable` listings, never test runs
   ([`prepare_and_execute` L597-L626](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L597-L626)).

**Answer.** Lookups: **yes**, against the RE action cache, when all three conditions from the summary hold. Uploads of local test executions: **never**.
This is unchanged on current `main` (`b8ae22e6`, 2026-09-24), where the same `NoOpCacheUploader` swap sits at `orchestrator.rs` L1531-L1532.
With a local-only executor, therefore, nothing inside buck2 ever writes a test `ActionResult`. A lookup can only hit an entry that something else wrote under the same action digest:

- **An RE worker that ran the same test.** Upstream's `test_remote_test_execution_cached` covers exactly this remote-only case ([`test_execution.py` L73-L95](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/tests/core/test/test_execution.py#L73-L95)).
- **An external writer to the action cache.**

| Executor config (OSS semantics) | Test opts in | Test result lookup | Local test result upload |
|---|---|---|---|
| `local_enabled=True, remote_enabled=False` (no `remote_cache_enabled`). This is turnkey today, see below. | any | none (`Executor::Local`) | none |
| … plus `remote_cache_enabled=True` | no | none (cache forced off) | none |
| … plus `remote_cache_enabled=True` | yes | RE action cache | none |
| … plus `remote_cache_enabled=True`, run with `--no-remote-cache` | yes | none | none |
| `remote_enabled=True` (remote or hybrid) | yes | RE action cache; RE-executed runs become later hits | none for local or fallback runs |

**Listings behave differently.** When a runner marks a listing `cacheable`
([`test.proto` L30-L33](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L30-L33)),
buck2 looks it up and, on a miss, calls `cache_upload` with the executor's real uploader
([`orchestrator.rs` L1042-L1108, L1303](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1042-L1108)).
So local *listings* can be uploaded when the executor has `allow_cache_uploads=True`.
The bundled OSS runner never sends a listing stage. It sends only `Testing` with empty `testcases`
([`runner.rs` L128-L133](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_runner/src/runner.rs#L128-L133)).

**`UploadFileToCas` does not create cache entries.** This RPC, added in
[3cd857208b](https://github.com/facebook/buck2/commit/3cd857208bc43366e61c6773cb776e91bd5a7cd0) and
[67c2f472f8](https://github.com/facebook/buck2/commit/67c2f472f8baba4b1f961de2c78f7382b300e1ea), lets a runner push a local *file* into CAS and get its digest back
([`test.proto` L342-L351, L369](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L342-L351)).
It stores blobs, not `ActionResult`s.

**Turnkey today.** `.buckconfig` sets `execution_platforms = prelude//platforms:default` ([`nix/devenv/turnkey/buck2.nix` L367](../../nix/devenv/turnkey/buck2.nix#L367)).
That platform is `CommandExecutorConfig(local_enabled = True, remote_enabled = False, …)`
([`buck2-prelude@27c8628d` `platforms/defs.bzl` L21-L25](https://github.com/facebook/buck2-prelude/blob/27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795/platforms/defs.bzl#L21-L25)),
so it becomes `Executor::Local`. There are no action cache lookups for tests or builds, and no uploads.

## 3. Ordinary `run` actions

### 3a. `allow_cache_uploads` / `allow_cache_upload`

**Executor gate.** Both conditions are needed:

- `CommandExecutorConfig(allow_cache_uploads = True)`, whose default is `False`
  ([L120, L159](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L159)).
  It becomes `CacheUploadBehavior::Enabled { max_bytes }`, optionally capped by `max_cache_upload_mebibytes`
  ([L294-L306](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L294-L306)).
- Caching must not be disabled: `remote_cache_enabled=True` and no `--no-remote-cache`. Otherwise the factory installs `NoOpCacheUploader`
  ([`common.rs` L426-L452](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L426-L452)).

A local executor with remote cache on and `allow_cache_uploads=True` therefore gets a real uploader.

**Action gate.** `ctx.actions.run(allow_cache_upload = …)` defaults to `None`
([`context/run.rs` L269](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/context/run.rs#L269)).
`None` falls back to `buck2.default_allow_cache_upload`, which defaults to `false`
([`run.rs` L1549-L1552](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1549-L1552);
[`ctx.rs` L402, L763-L768](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/ctx.rs#L763-L768)).
The prelude sets the value per action in places, for example
[`rust/build.bzl` L667](https://github.com/facebook/buck2-prelude/blob/27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795/rust/build.bzl#L667).

**When an upload happens.** buck2 calls `cache_upload` when all of these hold
([`run.rs` L1565-L1588](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1565-L1588)):

- the action succeeded;
- the result was not served by the remote dep-file cache;
- `allow_cache_upload`, remote dep files, or `BUCK2_TEST_FORCE_CACHE_UPLOAD` is set.

`CacheUploader::upload` then writes an `ActionResult` **only if `was_locally_executed()`**
([`caching.rs` L576-L608](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L576-L608)).
It first applies the size cap and a write-permission probe, then uploads the action and output blobs and calls `write_action_result`
([L104-L181](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L104-L181)).

### 3b. `LocalActionCache` and dep-file reuse (in memory only)

- **Storage.** A process-global `DEP_FILES: BuckDashMap<RunActionKey, Arc<DepFileState>>` holds one entry per action
  ([`dep_files.rs` L94-L95](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L94-L95),
  key from [L664](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L664)).
- **Identical-action check, before the RE cache.** `check_local_dep_file_cache_for_identical_action` runs first
  ([`run.rs` L1046-L1054](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1046-L1054)).
  A hit requires the same dep-file declaration, reusable outputs, the same command-line digest and worker digest, and the same input-directory digest
  ([`dep_files.rs` L836-L877](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L836-L877)).
  The materializer must also confirm the outputs on disk still match
  ([L807-L834](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L807-L834)).
  Hits are reported as `ActionExecutionKind::LocalActionCache`
  ([L474-L509](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L474-L509)).
- **Dep-file check, after an RE action-cache miss.** If only dep-file-filtered inputs changed, a hit is reported as `LocalDepFile`
  ([`run.rs` L1071-L1078](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1071-L1078);
  [`dep_files.rs` L511-L547](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L511-L547)).
- **Population.** After **every** `run` action, including ones without dep files (`has_declared_dep_files: None`) and results served by RE, with a `was_produced_locally` tag
  ([`dep_files.rs` L1053-L1128](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L1053-L1128);
  [`run.rs` L1618](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1618)).
- **Lifetime.** Process memory only, and flushable
  ([L103-L129](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run/dep_files.rs#L103-L129)).
  At the pin, `app/buck2_execute_impl/src/sqlite/` holds only the materializer and incremental-state databases, and the only sqlite buckconfigs are
  `buck2.sqlite_materializer_state` and `buck2.sqlite_incremental_state`
  ([`disk_state.rs` L57, L162](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/disk_state.rs#L57)).
- **Tests don't use it.** The orchestrator never touches dep-file state.

### 3c. `buck2.sqlite_dep_file_state`: absent at the pin

- `git grep sqlite_dep_file_state 7600cb80…` finds nothing. On current `main` the key lives in `app/buck2_server/src/daemon/disk_state.rs`.
- **Timeline** (none of it is an ancestor of the pin):
  - Types and database: [16c8a75e55](https://github.com/facebook/buck2/commit/16c8a75e5513ce42552aba1521447c732e823328) and [2ca2802708](https://github.com/facebook/buck2/commit/2ca2802708b96e064fbb27be7a2f2685cbef22a4), both 2026-07-31.
  - Wiring: [9944684e95](https://github.com/facebook/buck2/commit/9944684e952be7de56507ec1dd3759e9759220a6), 2026-08-11. It adds the `buck2.sqlite_dep_file_state` key, default off, and ignores it unless `buck2.sqlite_materializer_state` is also on.
  - Per the commit message, entries reloaded after a restart serve only the identical-action (`LocalActionCache`) path, not the dep-file-filtered one.
  - Follow-ups continue through a06e30e4ae (2026-09-17).
- **Releases:** `2026-08-01` has the database but not the key. `2026-08-22`, `2026-09-01` and `2026-09-15` have both. No release yet contains a06e30e4ae.

## 4. `test.proto` at the pin vs quokka's vendored copy

- **Identical.** buck2 `app/buck2_test_proto/test.proto` at the pin is git blob `18ca8a5847824ab6feebe0599e66014832ee8e3d`, sha256 `b0058842…`.
  quokka's [`proto/test.proto`](https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/proto/test.proto) at `bd0bd215` (2026-08-07) has the same blob and sha256, and `diff -u` prints nothing.
- **Why.** Upstream `test.proto` did not change between [382a94826a](https://github.com/facebook/buck2/commit/382a94826a897d3eeb7ae9a430360677eddda623) (2026-03-26) and [ddfb9b6c39](https://github.com/facebook/buck2/commit/ddfb9b6c392175ab1d993f17cd817bd3156fcd76) (2026-06-02).
  quokka runs buck2 `2026-06-01` (`b2532a07`, [`nix/buck2.nix`](https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/nix/buck2.nix)) and cites `3f054b09` (2026-05-18). Both carry the same blob.
- **Other protos.**
  - `host_sharing.proto` and `downward_api.proto` are identical to the pinned `app/buck2_host_sharing_proto/host_sharing.proto` and `app/buck2_downward_api_proto/downward_api.proto`.
  - `data.proto` differs **by design**. quokka ships a minimal, wire-compatible subset of `app/buck2_data/data.proto` ([header L1-L16](https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/proto/data.proto#L1-L16)).
    Checked against the pin, every field it declares has the same number and type: `LocalCommand`, `OmittedLocalCommand`, `WorkerInitCommand`, `WorkerCommand`, `RemoteCommand` (fields 1, 2 and 5), `CommandExecutionKind` and `CacheHitType`.
    It omits `RemoteCommand` fields 6-8 (`remote_dep_file_key`, the `materialized_*` lists) without mentioning them. That is harmless, because proto3 skips unknown fields.
- **Drift from current `main`.** There is one additive field, `optional buck.data.CommandExecution command_execution = 9;` in `ExecutionResult2` ([ddfb9b6c39](https://github.com/facebook/buck2/commit/ddfb9b6c392175ab1d993f17cd817bd3156fcd76)).
  It is wire-compatible. Consuming it would need `CommandExecution` added to the `data.proto` subset.
- **Caching-relevant contents of the pinned `test.proto`:**
  - `TestStage.Listing.cacheable` ([L30-L33](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L30-L33))
  - `ExecuteRequest2.disable_test_execution_caching = 11` ([L187](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L187))
  - `ExecutionDetails.execution_kind` ([L294-L296](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L294-L296))
  - `UploadFileToCas` ([L342-L351, L369](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L342-L351))
  - The `Output` TODO "when we start uploading results of local executions to CAS" ([L269-L276](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L269-L276))

## 5. How toolbox produces buck2, patching it, and bumping it

### Production: prebuilt upstream binary

The package lives in `firefly-engineering/toolbox`, [`packages/buck2/default.nix`](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2/default.nix).
Its `default` builder is a `stdenv.mkDerivation` that works as follows
([L15-L54](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2/default.nix#L15-L54)):

- `src` is a `fetchurl` of `https://github.com/facebook/buck2/releases/download/${version}/buck2-${targetTriple}.zst`;
- `dontUnpack`, `dontConfigure` and `dontBuild` are all set;
- the install phase runs `zstd -d $src -o $out/bin/buck2`;
- on Linux, `autoPatchelfHook` patches the binary.

Per-platform hashes live in [`data.json`](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2/data.json).

Upstream builds those release binaries on GitHub Actions from `main`, using `cargo build --release --bin buck2 --bin rust-project` with `RUSTFLAGS="--cfg tokio_unstable -C strip=debuginfo -C codegen-units=1"`
([`upload_buck2.yml` L118-L138](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/.github/workflows/upload_buck2.yml#L118-L138)).
Each release also publishes a `prelude_hash` asset
([L25-L44](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/.github/workflows/upload_buck2.yml#L25-L44)).

Other Nix packagings take the same approach:

- nixpkgs' `buck2` at turnkey's locked nixpkgs `f8573b9c` is also a release download (`pkgs/by-name/bu/buck2/package.nix`, version `unstable-2025-12-01`, found with a narrow `nix eval`).
- quokka overrides that nixpkgs package's `srcs` to a newer release ([`nix/buck2.nix`](https://github.com/njaremko/quokka/blob/bd0bd215f7948226928a02ddb061a32a0f1668bd/nix/buck2.nix)).

### How the registry wiring constrains an override

- **toolbox's registry is a closed fixpoint.** Every package receives toolbox's own package set
  ([toolbox `flake.nix` L65-L77](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/flake.nix#L65-L77)).
  `buildToolchain` symlink-joins `toolbox.${component}.versions.${ver}`
  ([`lib/default.nix` L43-L57](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/lib/default.nix#L43-L57)).
  Overriding the `buck2` entry downstream therefore does **not** change what `buck2-toolchain` version `"3"` contains.
- **turnkey's `registryExtensions` merge version by version.** New versions are added and an explicit `default` overrides
  ([`nix/flake-parts/turnkey/default.nix` L667-L705](../../nix/flake-parts/turnkey/default.nix#L667-L705)).
  turnkey can add, say, `buck2-toolchain.versions."3-patched"` (a patched buck2 plus reindeer) without forking toolbox, then select it in `toolchain.toml`.

### Carrying a buck2 patch

- **Not possible on the current derivation.** It has no source and no unpack or build phase, so `patches` has nothing to apply to.
- **A from-source derivation would need, at the pin:**
  - **Nightly Rust** `nightly-2026-01-18` ([`rust-toolchain` L13-L15](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/rust-toolchain#L13-L15)). `fenix` is already a transitive input of turnkey through `nix-pins`.
  - **A self-generated `Cargo.lock`.** Upstream does not commit one: `Cargo.lock` is in [`.gitignore` L2](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/.gitignore#L2) and none exists at the root. The lock must be generated and vendored next to the patch. The four git dependencies need output hashes ([`Cargo.toml` L272, L359, L514-L515](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/Cargo.toml#L272)).
  - **`--cfg tokio_unstable`**, from [`.cargo/config.toml` L1-L5](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/.cargo/config.toml#L1-L5). It must be kept if `RUSTFLAGS` is overridden ([upstream `flake.nix` L48-L51](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/flake.nix#L48-L51)).
  - **`protoc`**, either vendored on tier-1 platforms ([`HACKING.md` L54-L69](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/HACKING.md#L54-L69)) or supplied through `BUCK2_BUILD_PROTOC`/`BUCK2_BUILD_PROTOC_INCLUDE` ([`flake.nix` L44-L45](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/flake.nix#L44-L45)).
  - **Optionally**, `BUCK2_SET_EXPLICIT_VERSION` to stamp `--version`.

  Upstream itself says "A Nix package … does not yet exist" ([`HACKING.md` L46-L47](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/HACKING.md#L46-L47)).
- **Same code paths as the release.** The GitHub tree hardwires `is_open_source()` to `true` ([`buck2_core/src/lib.rs` L73-L86](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_core/src/lib.rs#L73-L86)). A cargo build from GitHub source therefore takes the same OSS paths as the release binary. The "Cargo build detected: disabling remote execution and caching" branch fires only for Meta-internal source ([`common.rs` L208-L226](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L208-L226)).
- **Natural home.** toolbox already supports a per-version `builder` choice ([`lib/default.nix` L29-L39](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/lib/default.nix#L29-L39)) and has a `resolvePatches` helper for a `patches` list in `data.json` ([L59-L64](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/lib/default.nix#L59-L64)).
  A `source` builder for `buck2` with a patch list would match turnkey's "fetch upstream and patch" rule. The alternative is a `registryExtensions` entry in turnkey, as described above.

### Bumping to a newer buck2

1. **In toolbox:**
   - Add a `packages/buck2/data.json` entry for the release tag, with four sha256 hashes.
   - Add a `buck2-toolchain` version `"4"`.
   - Ideally, add a `buck2-prelude` entry whose `rev` is that release's `prelude_hash` asset (see the side finding below).
2. **In turnkey:** run `nix flake update toolbox`, then set `buck2-toolchain = { version = "4" }`.
3. **Candidate release:** the latest is `2026-09-15` (`6507dd157a`). It contains `buck2.sqlite_dep_file_state`.
4. **Upstream changes between the pin and `2026-09-15` that affect rules:**
   - `ExternalRunnerTestInfo.network_access` was **removed** ([a4cbaaa912](https://github.com/facebook/buck2/commit/a4cbaaa912c025673b2e80c7911af8ea008ea436), 2026-06-16), so any rule passing `network_access=` breaks.
   - `BuckInternalRunnerTestInfo` was added for in-process test execution ([59018bb3d9](https://github.com/facebook/buck2/commit/59018bb3d91f7738f773d66cef83fffb71146228)).
   - `test.proto` gained field 9 (additive).
   - Test result caching semantics are unchanged: there are still no uploads of local test executions.

## Side finding: the prelude is one release behind the binary

- **What turnkey uses.** turnkey resolves `buck2-prelude` from the registry default ([`nix/flake-parts/turnkey/default.nix` L707-L713](../../nix/flake-parts/turnkey/default.nix#L707-L713)). That is toolbox's `2026-03-15`, which is `facebook/buck2-prelude@27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795` ([toolbox `packages/buck2-prelude/data.json` L2-L5](https://github.com/firefly-engineering/toolbox/blob/9b190de95aa36df3215cfc239c44668f5ec47109/packages/buck2-prelude/data.json#L2-L5)).
  The upstream store path behind `.turnkey/prelude` hashes to the same `sha256-jTr/I75V…`.
- **What the releases pair it with.** The `prelude_hash` asset of release `2026-03-15` is `27c8628d…`. For release `2026-04-15`, the pinned binary, it is `f0896771c4cc1ab8f87e032c5293376c89e5096b`.
- **Consequence.** The prelude and the binary are paired one release apart. Any work on test rules (for example setting `supports_test_execution_caching`) should bump the two together.
