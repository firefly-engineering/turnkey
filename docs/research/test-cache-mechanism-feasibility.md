# Feasibility of three test-result-caching mechanisms on the pinned buck2

Research for ticket `turnkey-w55.7`: *Which mechanism can record and reuse test results with our pinned buck2?*
It covers option **E** (a custom test runner that writes `ActionResult`s itself), option **B** (tests as build actions) and option **D** (patching buck2).

- **Researched:** 2026-09-25.
- **Sources:** primary sources only: the buck2 source at the pinned commit, buck2's GitHub issues and PRs, and turnkey's own research notes. Nothing was built or run.
- **Pinned buck2:** [`7600cb80070a88b88be67aa5d20d6a93cffa0223`](https://github.com/facebook/buck2/commit/7600cb80070a88b88be67aa5d20d6a93cffa0223), release `2026-04-15`. See [`buck2-pinned-test-caching.md`](buck2-pinned-test-caching.md) for how the pin was identified and for the basic lookup and upload wiring. This note builds on that one and does not repeat it.
- Unless stated otherwise, every buck2 link is a permalink at the pin. Statements not read directly off the source are marked *(inferred)*.

The executor that option E assumes is `CommandExecutorConfig(local_enabled = True, remote_enabled = False, remote_cache_enabled = True)`, pointed at a local bazel-remote. Call it the **cache-read executor**.

## Summary

| # | Question | Answer at `7600cb80` | Key source |
|---|---|---|---|
| E1 | Does the runner receive buck2's action digest for a local run? | **Yes.** `ExecutionResult2.execution_details.execution_kind` is `LocalCommand { action_digest, argv, env }`. It is filled for every local run, and it is the same `ActionDigest` object the lookup uses. It is formatted `<hex>:<size>`. The runner must reuse buck2's `instance_name`. The use case is not sent in OSS builds. | [`orchestrator.rs` L371](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L371), [`local.rs` L1227-L1231](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/local.rs#L1227-L1231), [`kind.rs` L114-L136](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L114-L136) |
| E2 | Is the Action always computed, and is the lookup keyed by that digest? | **Yes, both.** `prepare_action` runs for every test request under every executor, `Executor::Local` included. The cache checker looks up `prepared_action.action_and_blobs.action`, and the local executor reports `prepared_action.digest()`, which is that same field. | [`orchestrator.rs` L1028](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1028), [`action_cache.rs` L245](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L245), [`prepared.rs` L38-L41](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/prepared.rs#L38-L41) |
| E3 | What must a hit's `ActionResult` contain? | **`execution_metadata` must be present**, or the lookup fails with an error that reaches the test as a failure. A non-zero `exit_code` is **replayed** as a failed test. stdout and stderr can be raw or a digest (non-empty raw wins). Output paths are **project-relative**. An output missing from the result is silently left out, not flagged *(inferred)*. With bazel-remote's default AC→CAS check, every referenced blob must be in CAS. | [OSS `client.rs` L1083-L1139](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L1083-L1139), [`download.rs` L146-L212, L387-L484](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/re/download.rs#L146-L212) |
| E4 | Does the runner see that a result was a hit, and the original duration? | **Yes, both.** A hit arrives as `RemoteCommand { cache_hit: true, cache_hit_type: ACTION_CACHE, action_digest }`. `execution_time` is `execution_completed_timestamp − execution_start_timestamp`, and `start_time` is `execution_start_timestamp`, both from the stored metadata. | [`kind.rs` L158-L167](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L158-L167), [`remote_action_result.rs` L146-L151, L174-L207](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/re/remote_action_result.rs#L174-L207) |
| E5 | With `disable_test_execution_caching = true`, is the digest still computed and reported? | **Yes, and it is the same digest.** The lookup is skipped, but `prepare_action` still runs, and the RE platform that enters the digest does not depend on the cache flag. `--no-remote-cache` behaves the same way. | [`orchestrator.rs` L1248-L1284, L1028](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1248-L1284), [`common.rs` L456](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L456) |
| E6 | What RE client config does the cache-read executor need? Does a cache-only endpoint work? | **`[buck2_re_client] address = grpc://127.0.0.1:9092` and `tls = false`.** `engine_address` is *required*, because the Capabilities and Execution channels are built from it at connect time. Pointing it at bazel-remote works, since Execution is never called. SHA256 is already the OSS default. The connection is lazy, and connect errors are cached *(inferred)*. | [`buck2_re_configuration` L498-L553](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_re_configuration/src/lib.rs#L498-L553), [OSS `client.rs` L302-L434](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L302-L434) |
| E7 | Do build actions also start looking up the cache? | **Only if the cache flag is set on the execution platform.** A test rule can instead set it on `ExternalRunnerTestInfo(default_executor = …)`. The orchestrator prefers that over the execution platform, so build actions stay on `Executor::Local`. On the platform, every `run` action would do a lookup, and a cache error would fail the action. | [`orchestrator.rs` L1396-L1425](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1396-L1425), [`external_runner_test_info.rs` L90-L93](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L90-L93) |
| E8 | Does the executor change path rendering? | **No.** Rendering depends only on `use_project_relative_paths` and `run_from_project_root` (or the `--unstable-allow-all-tests-on-re` force). Absolute paths go into `Command.arguments`, so the digest is only reusable within one checkout. | [`orchestrator.rs` L1458-L1501](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1458-L1501) |
| B9 | Is there a flag that skips cache reads but still uploads local results? | **No supported one.** `--no-remote-cache` turns local uploads off too. `--write-to-cache-anyway` only affects RE *workers*. `--upload-all-actions` uploads Action blobs, not results. Only the self-test env var `BUCK2_TEST_FORCE_CACHE_UPLOAD` gives "no reads, still write". | [`build.rs` L142-L167, L268-L273](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client_ctx/src/common/build.rs#L142-L167), [`common.rs` L278-L284, L426-L452](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L426-L452) |
| B10 | Do `run` actions have a timeout, and what environment do they get? | **`timeout_seconds` exists** (off by default) and goes into the digest. The environment is the **daemon's environment inherited**, minus `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `LD_LIBRARY_PATH` and `LD_PRELOAD`, plus `TMPDIR`, `BUCK2_DAEMON_UUID`, `BUCK_BUILD_ID` and `PWD`. Tests instead get a cleared environment with an allowlist. | [`context/run.rs` L195-L204](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/context/run.rs#L195-L204), [`environment_inheritance.rs` L19-L28, L76-L119](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/environment_inheritance.rs#L76-L119) |
| D11 | How big is a patch that uploads passing local test runs? Is there upstream discussion? | **About 30-40 lines in one file** (`orchestrator.rs`) *(inferred)*: keep the real uploader for opted-in `Testing`, and copy the listing arm's `cache_upload` call. The existing `CacheUploader` already uploads only successful local runs. Upstream: [#183](https://github.com/facebook/buck2/issues/183) has been open since 2023. No upstream PR exists. A downstream fork carries a different, in-memory DICE patch. | [`orchestrator.rs` L1086-L1107, L1302-L1315](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1086-L1107), [`caching.rs` L451-L470, L576](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L451-L470) |

**Bottom line for E.** It is feasible without patching buck2:

- buck2 computes the digest and hands it to the runner.
- It looks that digest up natively, and reports hits and their original timing back to the runner.
- The runner has everything it needs to write the entry: the digest, stdout and stderr inline, and output paths on disk.

It has to meet five conditions:

- write under buck2's exact `hash:size` digest, with the same `instance_name`;
- always set `execution_metadata`;
- write only passing runs;
- name outputs by their project-relative path;
- upload every referenced blob, including `Tree` messages, before the AC write.

## E. A runner that writes `ActionResult`s itself

### E1. The runner gets the action digest for local runs

- **Where it goes.** `execute2` builds `ExecutionResult2.execution_details.execution_kind` from `execution_kind.map(|k| k.to_proto(false))`
  ([`orchestrator.rs` L366-L373](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L366-L373)).
  The field is `optional buck.data.CommandExecutionKind execution_kind = 1`
  ([`test.proto` L294-L296](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L294-L296)).
- **Local runs are populated.** `LocalExecutor::exec_cmd` stamps `CommandExecutionKind::Local { digest: command.prepared_action.digest(), command, env }` before it runs anything
  ([`local.rs` L1227-L1231](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/local.rs#L1227-L1231)).
  The enum's comment reads "Even though this did not run on RE, we still produced this, so we might as well report it"
  ([`kind.rs` L28-L36](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L28-L36)).
  Because `omit_details` is `false`, it serialises to `LocalCommand { argv, env, action_digest }` rather than `OmittedLocalCommand`
  ([`kind.rs` L114-L136](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L114-L136);
  [`data.proto` L2351-L2356](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_data/data.proto#L2351-L2356)).
- **Format.** The string is `<lowercase hex>:<size_bytes>`
  ([`cas_digest.rs` L452-L462](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_common/src/cas_digest.rs#L452-L462), [L104-L108](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_common/src/cas_digest.rs#L104-L108)).
  The runner splits it into an REAPI `Digest { hash, size_bytes }`. The algorithm is SHA256: it is the OSS default unless `[buck2] digest_algorithms` or `BUCK_DEFAULT_DIGEST_ALGORITHM` overrides it
  ([`state.rs` L347-L371](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/state.rs#L347-L371)).
- **Instance name must match.** The OSS client sends `instance_name` from `[buck2_re_client] instance_name` on `GetActionResult` and `UpdateActionResult`. An unset value is sent as `""`
  ([OSS `client.rs` L250-L266, L681-L731](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L681-L731)).
  The runner should write under the same name. bazel-remote ignores instance names for the AC unless `--enable_ac_key_instance_mangling` is set ([`local-re-api-servers.md`](local-re-api-servers.md) §bazel-remote).
- **Use case need not be replicated.** The OSS client puts `use_case_id` on the wire only when `use_fbcode_metadata` is set. Otherwise it sends a plain REAPI `RequestMetadata`
  ([OSS `client.rs` L1725-L1760](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L1725-L1760)).
  The use case is reported back to the runner only as `RemoteCommandDetails.use_case`, and only on hits.

### E2. The Action is always built, and the lookup uses exactly that digest

- **Always built.** `execute_request` calls `executor.prepare_action(&request, digest_config, false)` before it branches on the stage. This happens under every executor, `Executor::Local` included
  ([`orchestrator.rs` L1028-L1031](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1028-L1031)).
  The comment at L1026 ("we currently do not do any cache queries") is stale, as the earlier note found.
- **What goes into it.** `prepare_action` builds an REAPI `Command` and `Action` from:
  - `all_args`, the working directory and `request.env()`;
  - the output paths;
  - the executor's RE platform properties;
  - the input root digest;
  - `request.timeout()`, as `Action.timeout`.

  ([`command_executor.rs` L191-L258, L317-L395](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/command_executor.rs#L317-L395)).
  The runner sets the timeout on every `ExecuteRequest2`
  ([`test.proto` L182](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L182);
  [`orchestrator.rs` L1581-L1583](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1581-L1583)).
  **A runner that varies the timeout therefore varies the digest.**
- **Same object for lookup and report.** `ActionCacheChecker::maybe_execute` takes `command.prepared_action.action_and_blobs.action`
  ([`action_cache.rs` L245](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L245)).
  `PreparedAction::digest()` returns that same field
  ([`prepared.rs` L38-L41](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/prepared.rs#L38-L41)).
  Both the checker and the local executor get the same `PreparedCommand`
  ([`orchestrator.rs` L1122-L1135](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1122-L1135)).
  The lookup sends that digest unchanged
  ([`action_cache.rs` L97-L108](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L97-L108)).
- **What stays out of the digest.** The inherited allowlist environment, `TMPDIR` and `BUCK_BUILD_ID` are added by the local executor and are not part of `request.env()`
  ([`orchestrator.rs` L1573](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1573);
  [`local.rs` L626-L666](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/local.rs#L626-L666)).
  This is consistent with [`test-undeclared-inputs.md`](test-undeclared-inputs.md) §3.3.

### E3. What buck2 requires of a hit's `ActionResult`

- **`execution_metadata` is mandatory.** The OSS client's `convert_action_result` fails with "The execution metadata are not defined." when it is absent
  ([OSS `client.rs` L1083-L1086](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L1083-L1086)).
  Only `NOT_FOUND` counts as a miss. Any other error ends the lookup with `manager.error("remote_action_cache", …)`
  ([`re/client.rs` L1071-L1076](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/re/client.rs#L1071-L1076);
  [`action_cache.rs` L130-L136](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L130-L136)).
  The orchestrator reports that error to the runner as `Finished { exitcode: 1 }`, with the error text as stderr
  ([`orchestrator.rs` L1221-L1234](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1221-L1234)).
  **A malformed entry, or an unreachable cache, therefore shows up as a failed test, not as a re-run.** Output file entries also need a `digest`, and output directory entries need a `tree_digest` (L1088-L1129).
- **Exit code is replayed.** `download_action_results` returns `success` for `exit_code == 0` and `failure(…, Some(e))` otherwise
  ([`download.rs` L146-L212](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/re/download.rs#L146-L212)).
  The orchestrator maps `Failure` to `Finished { exitcode }`
  ([`orchestrator.rs` L1199-L1208](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1199-L1208)).
  The early-return branch for `intend_to_fallback_on_failure` (L106-L122) is set only by hybrid fallback, not by a local executor *(inferred)*.
  **The runner must write only passing runs.** That is also what buck2's own uploader does: it hard-codes `exit_code: 0` and uploads only `Success` local results
  ([`caching.rs` L451-L456](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L451-L456);
  [`result.rs` L290-L299](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/result.rs#L290-L299)).
- **stdout and stderr: raw or digest.** `ReStdStream::new` takes non-empty `*_raw` first, then `*_digest`, and otherwise treats the stream as empty
  ([`output.rs` L52-L58](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/output.rs#L52-L58);
  OSS mapping at [`client.rs` L1135-L1139](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L1135-L1139)).
  The lookup does not ask for inlining (`GetActionResultRequest` defaults, L688-L694), so digest-referenced streams must be in CAS.
  The runner already holds the bytes, because a local run returns them as `ExecutionStream::Inline`
  ([`orchestrator.rs` L1177-L1182](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1177-L1182)).
- **Outputs.**
  - **Path semantics.** On a hit buck2 inserts every `output_files[].path` and `output_directories[].path` into the *input directory tree*, which is rooted at the project root. It then extracts the declared outputs by their project-relative `output_paths()`
    ([`download.rs` L383-L484](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/re/download.rs#L383-L484)).
    **Names must therefore be project-relative**, for example `buck-out/v2/test/execution/<target>/<hash>/<name>`, even when the working directory is a cell root. buck2's own uploader names outputs the same way, with `output.path().to_string()`
    ([`caching.rs` L327-L344, L373-L377](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L327-L344)).
  - **Where the runner finds them.** After a local run, each output reaches the runner as `Output::LocalPath(<absolute path>)`
    ([`orchestrator.rs` L343-L345](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L343-L345)).
    The runner strips the project root, hashes the file or tree, and uploads it.
  - **What is declared.** Declared test outputs are the runner's `pre_create_dirs`, created before the run, plus any outputs referenced in `cmd` or `env`, whose parent is created
    ([`orchestrator.rs` L1504-L1508](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1504-L1508), [L1975-L1981](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1975-L1981)).
  - **No outputs.** A result with empty `output_files` and `output_directories` is valid.
  - **Missing expected outputs.** `extract_artifact_value` returning `None` just skips the entry (L464-L483). Nothing on the test path checks outputs, unlike build actions, which raise `MissingOutputs`
    ([`action_executor.rs` L760-L785](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/actions/execute/action_executor.rs#L760-L785)).
    A missing output is therefore silently absent from the runner's `outputs` map *(inferred)*.
  - **Blobs.** `Tree` messages are downloaded eagerly (L434-L444), and files are materialised on demand
    ([`orchestrator.rs` L353-L360](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L353-L360)).
    Both must be in CAS. bazel-remote's default AC→CAS check also answers "not found" if any of them is missing ([`local-re-api-servers.md`](local-re-api-servers.md) §bazel-remote, Integrity).
    Symlinks go in `output_symlinks` (L403-L408). buck2's own uploader refuses to upload them (`caching.rs` L405-L410).
- **Action and Command blobs are not needed for a hit.** The download path reads only the result and its output and stream blobs. The Action is uploaded only under `--upload-all-actions`
  ([`action_cache.rs` L112-L128](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L112-L128)).
  The runner does not have the `Action` or `Command` bytes anyway. buck2's uploader writes them because some servers "need to inspect the Action related to the ActionResult" (`caching.rs` L139-L148). bazel-remote's AC→CAS check covers outputs, trees and streams, not the Action ([`local-re-api-servers.md`](local-re-api-servers.md)), so this does not block it *(inferred)*.

### E4. How the runner learns about hits and their original duration

- **Hit flag.** On a hit, `ActionCacheChecker` sets `CommandExecutionKind::ActionCache { details }`
  ([`action_cache.rs` L246-L258](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L246-L258)).
  That serialises to `RemoteCommand { action_digest, cache_hit: true, cache_hit_type: ACTION_CACHE, details: { use_case, platform, … } }`
  ([`kind.rs` L158-L167](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/kind.rs#L158-L167)).
- **Original duration.** For an action cache result, `timing()` is `timing_from_re_metadata`, with input materialisation and queue time zeroed. It computes:
  - `execution_time = execution_completed_timestamp − execution_start_timestamp`;
  - `start_time = execution_start_timestamp`.

  ([`remote_action_result.rs` L146-L151, L174-L207](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/re/remote_action_result.rs#L146-L151)).
  `CommandExecutionMetadata::from_re_timing` copies them
  ([`result.rs` L190-L203](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/result.rs#L190-L203)),
  and the orchestrator forwards them as `ExecutionResult2.start_time` and `execution_time`
  ([`orchestrator.rs` L368-L369](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L368-L369)).
  **The runner should set both timestamps from the local run's `start_time` and `start_time + execution_time`.** buck2's own uploader does the same (`caching.rs` L459-L468).

### E5. Recording without reading (`disable_test_execution_caching = true`)

- **What the flag turns off.** With the flag set, effective caching is false
  ([`orchestrator.rs` L398-L399](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L398-L399)):
  - `executor_config_with_remote_cache_override` clones the `RemoteEnabled` options with `remote_cache_enabled = false`, keeping the `RemoteEnabled` variant (L1270-L1279);
  - `get_command_executor` installs a no-op checker (L1307-L1311).
- **The digest survives.** `prepare_action` still runs (L1028), and the local executor still reports `LocalCommand.action_digest`.
- **The digest is the same.** The platform that enters `Command.platform` comes from `self.0.re_platform` (`command_executor.rs` L200). The factory sets that from `remote_options.re_properties.to_re_platform()` whatever the cache flag
  ([`common.rs` L454-L461](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L454-L461)).
  The digest recorded on a forced run is therefore the key later runs look up.
- **Same for `--no-remote-cache`.** It sets `skip_cache_read` and disables the checkers the same way ([`common.rs` L278-L300](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L278-L300)).
- **One caveat.** For the cache-read executor, `Executor::Local` without the cache and `RemoteEnabled(Local)` produce identical digests only when `remote_execution_properties` is empty. That is the default: `re_properties.unwrap_or_default()` ([`command_executor_config.rs` L352-L357](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/command_executor_config.rs#L352-L357)).

### E6. RE client config for a cache-only bazel-remote

- **Keys.** All of them live in `[buck2_re_client]`
  ([`buck2_re_configuration/src/lib.rs` L498-L553](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_re_configuration/src/lib.rs#L498-L553)):
  - `address` fills `cas_address`, `engine_address` and `action_cache_address` unless each is given explicitly.
  - `tls` **defaults to `true`** (L526-L531). It must be `false` for a plain `grpc://` bazel-remote.
  - `instance_name`, `capabilities` (default on), `http_headers` and `tls_*` are optional.

  The user docs list the same keys ([`docs/users/remote_execution.md` L17-L49](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/docs/users/remote_execution.md#L17-L49)).
- **`engine_address` is required even without remote execution.** `build_and_connect` opens five channels:
  - CAS and ByteStream from `cas_address`;
  - Execution and Capabilities from `engine_address`;
  - ActionCache from `action_cache_address`.

  Each channel is `connect`ed eagerly ("No address" if unset), and each result is unwrapped with `?`
  ([OSS `client.rs` L310-L355, L359-L360, L415-L434](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/remote_execution/oss/re_grpc/src/client.rs#L310-L434)).
  Pointing `engine_address` at bazel-remote works *(inferred)*:
  - the TCP/h2 connect succeeds;
  - Capabilities is served;
  - `Execute` is only called by the RE executor, which the cache-read executor never builds ([`common.rs` L341-L345](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L341-L345)).

  **So OSS buck2 does support a cache-only endpoint**, as long as all three addresses point at it. buck2#1475 reports this setup in use (see [`local-re-api-servers.md`](local-re-api-servers.md)).
- **Digest algorithm.** SHA256 is already the OSS default (`state.rs` L350-L355), and SHA256 is the only algorithm bazel-remote supports.
- **Connection lifetime.** The client is created lazily, on first use, inside an `AsyncOnceCell<Result<…>>`
  ([`re/manager.rs` L115-L160](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/re/manager.rs#L115-L160)).
  A failed connect is therefore cached with the client, and every later lookup fails until the client is rebuilt (daemon restart or config change) *(inferred)*.
  Combined with E3, **bazel-remote must be running before the first opted-in test.** Otherwise opted-in tests report failures.

Minimal config *(inferred from the keys above)*:

```ini
[buck2_re_client]
address = grpc://127.0.0.1:9092
tls = false
```

### E7. Keeping build actions off the cache

- **The platform route affects builds.** If the cache flag is set on the *execution platform*, every `run` action on that platform becomes `RemoteEnabled(Local)` with an `ActionCacheChecker`, and `RunAction` consults it before running
  ([`run.rs` L1060-L1064](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1060-L1064);
  [`action_executor.rs` L465-L485](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/actions/execute/action_executor.rs#L465-L485), with no per-action gate).
  Side effects:
  - one AC round trip per `run` action that misses the in-memory `LocalActionCache`;
  - the RE connection is opened on the first action;
  - a cache error fails the action (E3);
  - nothing is written unless `allow_cache_uploads = True`, so lookups can hit only on entries some other writer created.
- **The test-only route avoids it.** The orchestrator resolves the test executor in this order
  ([`orchestrator.rs` L1396-L1425](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1396-L1425)):
  1. the runner's `executor_override` name, looked up in `ExternalRunnerTestInfo.executor_overrides`;
  2. otherwise `ExternalRunnerTestInfo.default_executor`
     ([`external_runner_test_info.rs` L90-L96, L161-L163](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs#L90-L96));
  3. only then the target's execution platform.

  A wrapper rule can re-emit the provider with `default_executor` set to the cache-read executor and `supports_test_execution_caching = True`, leaving the execution platform as plain `Executor::Local` for builds.
  Alternatively it can put the cache-read executor under a named `executor_overrides` entry, so the runner opts in per request through `ExecuteRequest2.executor_override`
  ([`test.proto` L172-L188](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test_proto/test.proto#L172-L188)).
  The prelude already builds `default_executor`/`executor_overrides` pairs for RE tests
  ([`prelude/tests/re_utils.bzl` L71-L134](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/prelude/tests/re_utils.bzl#L71-L134)).
- **Caveat.** An override must produce the same digest on every run. Switching an override that carries different `remote_execution_properties` changes `Command.platform`, and with it the key.

### E8. Path rendering does not depend on the executor

`expand_test_executable` picks `DefaultCommandLineContext` or `AbsCommandLineContext` from `test_info.use_project_relative_paths()`, or from `force_use_project_relative_paths`. It picks the working directory from `run_from_project_root`, or `force_run_from_project_root`
([`orchestrator.rs` L1458-L1501](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1458-L1501)).
The executor supplies only the path separator (`executor_fs`). The two force flags are both set by `buck2 test --unstable-allow-all-tests-on-re`
([`buck2_client/src/commands/test.rs` L347-L348](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client/src/commands/test.rs#L347-L348)).

Consequences:

- Rules that leave both flags `False` (turnkey's `go_test` and others, see [`test-undeclared-inputs.md`](test-undeclared-inputs.md) §3.2) render absolute paths into `Command.arguments`. Their digests are stable within one checkout but differ across checkouts or machines.
- Such tests are marked `supports_re = false`, which forces `LocalRequired` (L992-L995). The cache-read executor satisfies that, because it runs locally.

## B. Tests as build actions

### B9. No supported "write but don't read" flag

| Mechanism | Effect on local `run` actions | Source |
|---|---|---|
| `--no-remote-cache` (or `BUCK_OFFLINE_BUILD`) | Sets `skip_cache_read` and `skip_cache_write`. `skip_cache_read` sets `disable_caching`, which installs **both** a no-op checker **and** a `NoOpCacheUploader`. | [`build.rs` L142-L146, L272-L273](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client_ctx/src/common/build.rs#L142-L146); [`common.rs` L278-L284, L436-L437](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L278-L284) |
| `--write-to-cache-anyway` (requires `--no-remote-cache`) | Only clears `skip_cache_write`. That is read only by `ReExecutor`, which turns it into `do_not_cache` on the RE *Execute* request. It has no effect on local execution. | [`build.rs` L148-L150](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client_ctx/src/common/build.rs#L148-L150); [`common.rs` L245-L246](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L245-L246); [`re/client.rs` L1493-L1510](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/re/client.rs#L1493-L1510) |
| `--upload-all-actions` | On each cache lookup, uploads the Action blobs and input tree to CAS. It does not upload results, and it does nothing when lookups are disabled. | [`build.rs` L160-L167](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_client_ctx/src/common/build.rs#L160-L167); [`action_cache.rs` L112-L128](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/action_cache.rs#L112-L128) |
| `BUCK2_TEST_FORCE_CACHE_UPLOAD=true` (daemon env) | Installs a real `CacheUploader` **even when `disable_caching`**, as long as the executor is `RemoteEnabled`. It also bypasses the per-action `allow_cache_upload` gate. Combined with `--no-remote-cache` it gives "no reads, still write". It is registered with `applicability = testing`, meaning "Only used in self-tests of buck2". | [`common.rs` L426-L435](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_server/src/daemon/common.rs#L426-L435); [`cache_uploader.rs` L56-L62](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/cache_uploader.rs#L56-L62); [`run.rs` L1565-L1568](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1565-L1568); [`registry.rs` L14-L20](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_env/src/env/registry.rs#L14-L20) |

Under normal flags (reads on) a passing build action uploads when the executor has `allow_cache_uploads = True` and the action has `allow_cache_upload = True`, as described in [`buck2-pin` note §3a](buck2-pinned-test-caching.md#3a-allow_cache_uploads--allow_cache_upload). So B needs a "skip reads" mode only for a forced re-run.

### B10. `run` action timeout and environment

- **Timeout.** `ctx.actions.run(timeout_seconds = …)`:
  - is optional, a positive integer, and off by default ("The default is no timeout");
  - carries upstream advice not to use it to enforce runtime policy
    ([`context/run.rs` L195-L204, L287](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/context/run.rs#L195-L204));
  - becomes `req.with_timeout(timeout)`
    ([`run.rs` L1220-L1222](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1220-L1222));
  - is enforced by the local executor
    ([`local.rs` L361-L369, L857](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/local.rs#L361-L369));
  - is written into `Action.timeout`, so it is part of the digest (`command_executor.rs` L385-L388).

  A timed-out action is a failure, so it is never uploaded.
- **Environment of local `run` actions.** `RunAction` uses `EnvironmentInheritance::local_command_exclusions()`
  ([`run.rs` L1209](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_action_impl/src/actions/impls/run.rs#L1209)).
  That value has `clear: false` and removes `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `LD_LIBRARY_PATH` and `LD_PRELOAD`
  ([`environment_inheritance.rs` L107-L119](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute/src/execute/environment_inheritance.rs#L107-L119)).
  The action therefore **inherits the buck2 daemon's whole environment** apart from those five, and the daemon's environment is fixed when the daemon starts *(inferred)*. The local executor then adds:
  - `TMPDIR`, set to the action's scratch dir when there is one;
  - the declared `env`;
  - local-resource variables;
  - `BUCK2_DAEMON_UUID` and `BUCK_BUILD_ID`
    ([`local.rs` L626-L666](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/local.rs#L626-L666));
  - `PWD` (L1596-L1618).

  Only the declared `env` enters the digest.
- **Contrast with tests.** Tests use `test_allowlist()`, which is `clear: true` plus `PATH`, `USER`, `LOGNAME`, `HOME`, `TMPDIR` and `XDG_RUNTIME_DIR`, captured once per daemon process (L19-L28, L76-L103). **Moving a test into a `run` action exposes it to *more* undeclared environment, not less.**

## D. Patching buck2

### D11. Patch size and upstream status

The minimal patch is confined to `app/buck2_test/src/orchestrator.rs` and has two hunks *(inferred; not written or compiled)*:

1. **`get_command_executor`, Testing arm.** Keep the real `cache_uploader` when `supports_test_execution_caching`, instead of the unconditional `NoOpCacheUploader`
   ([L1302-L1315](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1302-L1315)). About 3-5 lines.
2. **`execute_request`, Testing arm.** Return `(result, cached)` from the span, as the Listing arm does. On a miss, call
   `executor.cache_upload(&CacheUploadInfo { target, digest_config, mergebase: &None, re_platform }, &result, None, None, &prepared_action.action_and_blobs)`,
   copying the Listing arm ([L1042-L1107](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1042-L1107) against [L1109-L1155](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_test/src/orchestrator.rs#L1109-L1155)). About 25-30 lines.

The existing `CacheUploader` already does the rest:

- **Passes only.** It uploads only when `was_locally_executed()`, which means `Success` with a `Local` kind, so failing tests are never written ([`caching.rs` L576](https://github.com/facebook/buck2/blob/7600cb80070a88b88be67aa5d20d6a93cffa0223/app/buck2_execute_impl/src/executors/caching.rs#L576)).
- **Blobs.** It uploads the Action blobs, the outputs (test paths resolve through `resolve_outputs`) and stdout/stderr, and rejects symlink outputs.
- **Result.** It writes `exit_code: 0` with real timestamps (L104-L181, L316-L470).
- **Gates.** The executor needs `allow_cache_uploads = True`. The optional size cap and write-permission probe apply.

No proto change is needed: the runner already sees hits (E4).

**Carrying the patch** requires the from-source buck2 derivation described in [`buck2-pinned-test-caching.md` §5](buck2-pinned-test-caching.md#5-how-toolbox-produces-buck2-patching-it-and-bumping-it).

**Upstream status** (`gh search`, 2026-09-25):

- **Main issue.** [facebook/buck2#183](https://github.com/facebook/buck2/issues/183), "Enable cached test results", has been open since 2023-04-19. Notable comments:
  - stepancheg (2023-04-20): "it does not cache test results. Maybe it should."
  - ndmitchell (2023-04-22) suggests running tests at build time in a custom rule. That is option B.
  - alexlian (2023-04-26) says Meta's internal opt-in "usually only cach[es] passing tests".
  - cjhopman (2024-01-25): "I don't think it'd be a particularly extensive change to support this".
  - njaremko (2026-06-09) points to quokka: a custom runner plus a wrapper rule, with RE doing the writes.
- **No PRs.** No upstream issue or PR proposes uploading *local* test executions. Searches for "test caching", "upload local test", "NoOpCacheUploader", "supports_test_execution_caching" and "test result cache" return nothing beyond #183. The upstream rationale for not uploading is in [d1d736ed45](https://github.com/facebook/buck2/commit/d1d736ed45a661aa86e2000a1d598cf6bde915e9) (cited in the earlier note). No GitHub Discussions matched.
- **A different approach downstream.** [facebook/buck2#1368](https://github.com/facebook/buck2/pull/1368) ("Mercury head next", opened and closed within seconds on 2026-07-10) is a MercuryTechnologies fork rebase. It lists a carried "test caching dice patch", [MercuryTechnologies/buck2@1fe6033965](https://github.com/MercuryTechnologies/buck2/commit/1fe6033965bc0d30ad3cd0a1be4f352fc08a1fc9) (2026-03-11): "Avoids re-running passed tests. Only compatible with DICE (local only)."
  It changes 8 lines in `orchestrator.rs`, memoising `Testing` on DICE and dropping the per-session prefix. That gives **in-memory, per-daemon** reuse, not action-cache writes. It predates the pin's stable test paths, so it would not apply as-is *(inferred)*.

## Not covered or not verified

- Nothing was executed. In particular, bazel-remote's acceptance of an `UpdateActionResult` whose Action blob is absent from CAS, and buck2's behaviour on a connection to bazel-remote as `engine_address`, are read from source only.
- The exact failure surface when the RE client's first connect fails (the cached `Err` in `AsyncOnceCell`) was not traced through config reloads.
- Line counts for the option D patch are estimates. No patch was written.

## Verification run (turnkey-w55.10, 2026-09-25)

The mechanism was run end to end once, on aarch64-darwin, with the pinned buck2 (`2026-04-14-7600cb80…`) and bazel-remote 2.6.2 from nixpkgs. Setup:

- a throwaway buck2 project with an empty prelude;
- a rule emitting `ExternalRunnerTestInfo(supports_test_execution_caching = True, use_project_relative_paths = True, run_from_project_root = True, default_executor = CommandExecutorConfig(local_enabled = True, remote_enabled = False, remote_cache_enabled = True))`;
- `[buck2_re_client]` with the engine, action cache and CAS addresses all set to `grpc://127.0.0.1:9092` and `tls = false`;
- buck2's bundled OSS runner.

Each test script appended to a side log when it really executed. That log is how "ran" was told apart from "hit". The "runner writes the result" step was played by hand: an `ActionResult` was PUT to bazel-remote's HTTP `/ac/<hash>`, under the digest buck2 reported in the event log.

| Item | Result |
|---|---|
| `engine_address` pointed at bazel-remote, with no Execution service (E6) | **Confirmed.** buck2 connects (`GRPC GETCAPABILITIES`) and does an AC lookup (`GRPC AC GET <digest> NOT FOUND`) for the opted-in test. The execution platform stays local-only, so build actions do no lookups. |
| A hand-written `ActionResult` for a pass is served as a hit | **Confirmed.** The test did not execute, and its recorded stdout was printed. The console showed `✓ Pass (42.0s)`, the original duration taken from `execution_metadata`. `buck2 log what-ran` shows executor `cache`. |
| bazel-remote's AC validation accepts the entry | **Confirmed** for an entry with inline `stdout_raw` and no CAS references, with HTTP AC validation on. Entries that reference CAS blobs were not tried. |
| Hit survives the same daemon, a daemon restart (`buck2 kill`, the pid confirmed gone), `buck2 clean`, and a second copy of the project at another absolute path | **Confirmed**, with project-relative paths. |
| A recorded failure (`exit_code = 1`) | **Replayed as `✗ Fail` without executing.** This confirms that only passes may ever be written. |
| An entry without `execution_metadata` is reported as a failure (E3) | **Refuted.** It was served as `✓ Pass (0.0s)`. Metadata is still needed for the original duration, but a missing one doesn't fail the test. |
| Cache server down, fresh daemon | buck2 retries the connection with growing back-off for about **45 s**, then reports the test as **`✗ Fail` (exit 32)**, with `Remote Execution Error on REClientBuilder` in the details. The test does not execute. |
| Cache server killed under a daemon that had connected | Same result: about 45 s, then `✗ Fail`. |
| A failed connect stays failed for the daemon's life (E6) | **Refuted.** After the server came back, the same daemon hit normally on the next command. |
| Reads skipped (`--no-remote-cache`) while the server is down | The test **executes locally and passes**, with no connection attempt, and buck2 still reports the **identical action digest**. So "run uncached" is a working fallback, and "force a re-run, still record" has the digest it needs. The runner's per-request `disable_test_execution_caching` is expected to behave the same (E5); the OSS runner can't set it, so it wasn't tried directly. |
| A missing output is silently left out (E3) | **Not tested.** The OSS runner requests no outputs. |

Consequence for the design: an unreachable cache turns every opted-in test into a slow failure. The runner, or `tk`, therefore has to check that the cache is reachable **before** a run, and send `disable_test_execution_caching` (or pass `--no-remote-cache`) when it isn't.
