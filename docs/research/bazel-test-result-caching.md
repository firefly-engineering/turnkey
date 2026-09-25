# Bazel test result caching: when is a test result reusable?

Research note for ticket `turnkey-w55.4`. Bazel caches test results by default,
so it is the reference model for what users expect from `tk test`. This note
describes what Bazel actually does, based only on primary sources:

- **Bazel source** at the **9.2.0** release commit
  [`8220c61`](https://github.com/bazelbuild/bazel/tree/8220c6198837d5c13d53fea211cf3282aa12408a)
  (2026-07-13). Behaviour that differs on `master` (checked at
  [`151450c`](https://github.com/bazelbuild/bazel/tree/151450cd52144e37a07ba2193763d8812ddbe12b),
  2026-09-24) is called out where it happens.
- **Remote Execution API** at
  [`adbf4a2`](https://github.com/bazelbuild/remote-apis/tree/adbf4a27c86fbea4a37637a6cbcacef372406fe7).
- **bazel.build documentation.** Quotes were checked against the live pages on
  2026-09-25.
- **bazelbuild/bazel issues, PRs and commits**, used as evidence for pitfalls.

Claims inferred from reading the code, rather than stated by a doc or a test,
are marked *(inferred)*.

---

## The reuse policy in eight rules

Each rule summarises Bazel's effective behaviour. The **Lever** line names the
design choice a buck2-based `tk test` could copy or deliberately change.

1. **One test execution is the unit of reuse.** An execution is one target ×
   configuration × shard × run. Its key covers everything Bazel passes to the
   test process:
   - the content of the executable and its runfiles;
   - argv: the rule's `args` plus `--test_arg`;
   - the resolved environment, including the *values* of explicitly inherited
     variables;
   - `--test_filter`, `--run_under`, size and timeout, the shard and run index,
     and coverage mode;
   - the execution platform and its `exec_properties`.

   Anything the test uses without declaring it is outside the key: host tools,
   the home directory, the network, the clock. [§1](#1-the-key)
   *Lever:* adopt the "everything the process sees" key. Make it hard to use
   undeclared state, rather than trying to key it.

2. **There are two tiers with different keys and different memories.**
   - The **local action cache** holds one entry per output path, so only the
     last result per test. That entry records the action key, the input
     digests and the inherited environment values, plus a `test.cache_status`
     file with the previous verdict.
   - The **disk and remote caches** are content-addressed by the REAPI `Action`
     digest (command, input root, timeout, platform). They can hold many
     results per test but know nothing about history.

   The two keys are computed separately and have drifted apart before:
   `--test_timeout` was in the remote key but missing from the local key until
   [#30576](https://github.com/bazelbuild/bazel/issues/30576).
   [§1.1](#11-two-caches-two-keys)
   *Lever:* decide deliberately whether a "previous verdict" memory exists, and
   derive every tier's key from a single definition.

3. **Only passes are shared. Failures stay local and are not reused by
   default.**
   - Bazel never uploads a result with a non-zero exit code or a non-`SUCCESS`
     status.
   - It treats a cached non-zero result from the disk cache, remote cache or
     remote executor as a miss.
   - Locally, every verdict is persisted. Under `auto`, a previous failure
     forces a rerun. Under `yes`, a failure can be reused, but only if it was
     a "user error" (non-zero exit, timeout, OOM). Infrastructure failures are
     never reused.

   The REAPI does not forbid servers from caching non-zero exits, so the
   client has to defend itself. [§5](#5-failures-in-the-cache)
   *Lever:* adopt as-is. It is the property users rely on most.

4. **The reuse policy is separate from the key.** `--cache_test_results`, the
   `external` tag and `--runs_per_test` decide whether caches are consulted at
   all. None of them is part of the key.
   - `auto`: reuse unless the test is `external`, `runs_per_test > 1`, or the
     last local result failed.
   - `yes`: reuse unless the test is `external`.
   - `no`: never reuse. [§2](#2---cache_test_results)

   *Lever:* adopt. Keeping policy out of the key means changing it never
   splits the cache.

5. **"Don't reuse" also means "don't publish", and the tags don't touch the
   local tier.**
   - When the policy rejects cached results, Bazel adds `no-cache` to the test
     spawn. That also stops the fresh result from being written to the disk or
     remote cache; Bazel's own `TODO` says this is unintended.
   - Tags and flags give finer control: `no-remote-cache` and `no-remote` (remote
     only), `no-cache` and `local` (disk and remote), `--noremote_accept_cached`
     (reads), `--noremote_upload_local_results` and `no-remote-cache-upload`
     (writes).
   - None of the caching tags (`no-cache`, `no-remote*`, `local`) bypasses the
     local action cache. Only the `external` tag, `--cache_test_results=no`,
     and `--runs_per_test > 1` under `auto` force a rerun.

   [§3](#3-opt-outs)
   *Lever:* deviate. Keep read and write opt-outs separate, and have one
   documented tag for "always run".

6. **Retries happen inside one test action, and flakiness is not a cache
   dimension.**
   - `flaky = True` and `--flaky_test_attempts` are not in the key.
   - A retry reruns the identical spawn. Failed attempts are never uploaded,
     and a passing attempt is uploaded as an ordinary pass.
   - The `FLAKY` verdict survives only in the local `test.cache_status`.
     Another machine, or the same one after `bazel clean`, gets a plain
     `(cached) PASSED` *(inferred)*.
   - Under `auto`, `--runs_per_test > 1` turns caching off completely.

   [§5.3](#53-flaky--true---flaky_test_attempts-and-caching)
   *Lever:* decide whether a pass that needed retries may be published, and
   whether it should carry a flaky marker.

7. **Hermeticity is assumed, not checked.**
   - Bazel pins what it can: `TZ=UTC`, a fixed `PATH` (strict action env, the
     default since 9.0), `HOME=$TEST_TMPDIR` (since 9.0), and an
     exec-root-relative `TEST_TMPDIR`.
   - Inherited environment *values* go into the key. Leaking client
     environment therefore costs hit rate, not correctness.
   - Tests that must talk to the outside world are expected to be tagged
     `external`.
   - Undeclared host dependencies produce stale hits. The docs acknowledge
     that hermeticity "is not enforced".

   [§6](#6-pitfalls)
   *Lever:* adopt. Sandboxing is what makes this rule less risky.

8. **A cache hit is a visible, first-class result.**
   - Cached results are printed like fresh ones, with a marker: `(cached)
     PASSED` or `(k/n cached)`.
   - They are left out of the "Executed N" count in `Executed N out of M
     tests`, and flagged in the Build Event Protocol (BEP) as `cached_locally` or
     `cached_remotely`.
   - Logs come from the original run: either the files already in the local
     output tree, or stdout downloaded from the CAS into `test.log`.
   - `test.xml` is either downloaded or regenerated by a separate spawn that is
     cached on its own. [§4](#4-reporting-a-hit)

   *Lever:* adopt.

---

## 1. The key

### 1.1 Two caches, two keys

A `bazel test` target turns into one `TestRunnerAction` per shard and per run
([`TestActionBuilder.java#L344-L443`][tab-loop]). Each action runs its test
through a *spawn*. Reuse is checked twice, against two different keys.

| Tier | Keyed by | Stores | Source |
|---|---|---|---|
| Local action cache (in the output base) | Primary output exec path → entry holding the action key, input metadata digests, effective client environment, and a salt | The last result for that path only | [`ActionCacheChecker.java#L533-L592`][acc-must], [`#L876`][acc-cachekey], [`ActionCacheUtils.java#L39-L47`][acu] |
| Spawn cache: `--disk_cache`, `--remote_cache`, remote execution | REAPI `Action` digest: `Command` digest + input-root digest + timeout + `do_not_cache` + platform + salt | Any number of results, content-addressed | [`RemoteExecutionService.java#L504-L584`][res-build], [`Utils.java#L431-L454`][utils-action] |

On a local hit, the action does not run at all. The test's previous verdict is
read back from `test.cache_status`, a `TestResultData` proto that is written at
the end of **every** run, pass or fail
([`TestStrategy.java#L309-L316`][ts-post], [`TestRunnerAction.java#L586-L597`][tra-save],
[`#L663-L701`][tra-hit]).

On a local miss, the action runs. Its test spawn first goes through the spawn
cache, which may serve it from the disk or remote cache
([`AbstractSpawnStrategy.java#L119-L167`][asps]).

The local entry is also salted. The salt covers whether a remote cache is
enabled and the value of `--remote_default_exec_properties`, so switching
`--remote_cache` or `--disk_cache` on or off invalidates local test hits
([`RemoteModule.java#L1200-L1222`][rm-salt]).

### 1.2 What goes into each key

The local action key is `TestRunnerAction.computeKey`
([`#L507-L537`][tra-key]) plus the execution platform and `exec_properties`
that `ActionKeyComputer.getKey` appends to every action key
([`ActionKeyComputer.java#L36-L57`][akc]). The cache entry also records the
digest of every input and the values of the inherited environment variables
([`ActionCacheChecker.java#L291-L300`][acc-env]).

The remote key is built from the actual spawn that `StandaloneTestStrategy`
creates ([`#L98-L145`][sts-spawn]). Its environment comes from `TestPolicy`
([`#L60-L102`][tp]). The spawn's arguments, environment, output paths and
platform form the REAPI `Command`. Its inputs form the input root.

| Factor | Local action cache | Disk / remote (REAPI `Action`) | Notes |
|---|---|---|---|
| Test executable + runfiles (content **and** layout) | Input digest. The runfiles tree is one input whose digest covers the mapping and every file digest ([`RunfilesArtifactValue.java#L87-L117`][rav], [`TestActionBuilder.java#L207-L233`][tab-inputs]) | Input-root Merkle tree | Also covers `test-setup.sh`, the XML generator and coverage tools, all inputs from `@bazel_tools` |
| Rule `args` + `--test_arg` | `executionSettings.getArgs()` in the key ([`TestTargetExecutionSettings.java#L71-L73`][tes-args]) | `Command.arguments` | |
| `--test_filter` | In the key | Passed as `TESTBRIDGE_TEST_ONLY` env ([`TestRunnerAction.java#L754-L757`][tra-filter]) | |
| `--test_runner_fail_fast`, `--run_under` | In the key | env / argv (+ the `--run_under` executable as an input) | |
| `--action_env`, `--test_env NAME=value`, rule `env` | Fixed values in the key (`ActionEnvironment.addTo`, [`#L134-L137`][ae-addto]) | `Command.environment_variables` | Applied in this order: `--action_env`, then `--test_env`, then the rule's env ([`TestPolicy.java#L88-L95`][tp-order]) |
| `--test_env NAME` (inherited), `env_inherit` | Only the *name* is in the key. The *value* is stored in the cache entry's "effective environment" ([`TestRunnerAction.java#L163-L169`][tra-clientenv], [`ActionCacheChecker.java#L291-L300`][acc-env]) | The resolved value is in `Command` | So a change of value in the client shell reruns the test on both tiers |
| `size`, `timeout` category | In the key | `TEST_SIZE` env | |
| Resolved timeout (`--test_timeout`) | **Not in the key in 9.2.0** ([#30576](https://github.com/bazelbuild/bazel/issues/30576)); added on master in [`a05f041`](https://github.com/bazelbuild/bazel/commit/a05f041e4714a324d23d731641674deeac9e2744) ([master `#L538`][master-timeout-line]), which is in `10.0.0-pre.20260806.4` and later but in no 9.x release | `Action.timeout`, from `ExecutionRequirements.TIMEOUT` ([`StandaloneTestStrategy.java#L120-L121`][sts-timeout]), and the `TEST_TIMEOUT` env | The REAPI puts the timeout in the `Action` so that "a lower timeout will result in a cache miss" ([proto `#L706-L715`][reapi-timeout]) |
| `tags` | All tags are in the key | Only indirectly, through `do_not_cache` and the salt | Only tags with certain prefixes become execution requirements ([`TargetUtils.java#L52-L62`][tu-legal]) |
| Shard index and count | In the key; one action per shard | `TEST_SHARD_INDEX`/`TEST_TOTAL_SHARDS` env; distinct output paths | `--test_sharding_strategy` changes the shard count |
| Run number / `--runs_per_test` | In the key; one action per run | When runs > 1, `TEST_RUN_NUMBER` and `TEST_RANDOM_SEED` env ([`TestRunnerAction.java#L738-L751`][tra-seed]); distinct output paths | `--runs_per_test` is itself a reason not to cache (§2) |
| Coverage mode, `--zip_undeclared_test_outputs` | In the key | Inputs, env and outputs differ | |
| Execution platform and `exec_properties` | `ActionKeyComputer` ([`#L48-L56`][akc-plat]) | `Platform` in `Command`/`Action`; the salt records whether the spawn may execute remotely ([`RemoteExecutionService.java#L370-L386`][res-salt]) | |
| Configuration (for example `-c opt`) | Through the output path (the local entry key) | Through `Command.output_paths` | |
| Bazel version | Only through a hard-coded GUID ([`TestRunnerAction.java#L110`][tra-guid]) and the content of bundled tools such as `test-setup.sh` | Bundled tools in the input root | |
| Tools found on `PATH`, system libraries | **Not tracked** | **Not tracked** | "Bazel currently does not track tools outside a workspace" ([caching known issues](https://bazel.build/remote/caching#known-issues); open [#4558](https://github.com/bazelbuild/bazel/issues/4558)) |

The following are **not in either key**. They are policy or reporting inputs
only:
- `flaky` and `--flaky_test_attempts`
  ([`TestStrategy.java#L272-L307`][ts-attempts], [`ExecutionOptions.java#L239-L260`][eo-flaky]);
- `--cache_test_results` (§2);
- `--test_keep_going`, `--test_output`, `--test_summary`;
- `--test_result_expiration`, which is deprecated and "has no effect"
  ([`TestConfiguration.java#L187-L194`][tc-expire]).

Changing any of these never invalidates a cached result.

The fixed parts of the test environment are:
- `TZ=UTC` and the runfiles and tmpdir variables
  ([`StandaloneTestStrategy.java#L76-L88`][sts-env]);
- `TEST_TMPDIR`, which is relative to the exec root and named after a hash of
  the executable path, shard and run
  ([`TestStrategy.java#L336-L343`][ts-tmpname]);
- the strict `PATH`, which is a fixed value by default in 9.x
  (`--incompatible_strict_action_env` defaults to `true`,
  [`BazelRuleClassProvider.java#L72-L87`][brcp-strict], flipped for 9.0 in
  [`60c5a03`](https://github.com/bazelbuild/bazel/commit/60c5a039040d9b228b3c6d6f316702cacb68d91f)).

Two things are set *inside* `test-setup.sh` and are therefore not in either
key: `HOME=$TEST_TMPDIR` (exported since 9.0.0,
[`03b0045`](https://github.com/bazelbuild/bazel/commit/03b00453bbf58a30e9c878b75196f58d1dd9a4d3)),
and `USER=$(whoami)` when it was not passed in
([`test-setup.sh#L64-L73`][setup]).

---

## 2. `--cache_test_results`

The flag's help text ([`TestConfiguration.java#L168-L186`][tc-cache]) and the
[user manual](https://bazel.build/docs/user-manual#cache-test-results) agree:

> If set to `auto`, Bazel reruns a test if and only if: 1. Bazel detects changes
> in the test or its dependencies, 2. The test is marked as `external`,
> 3. Multiple test runs were requested with `--runs_per_test`, or 4. The test
> previously failed. If set to `yes`, Bazel caches all test results except for
> tests marked as `external`. If set to `no`, Bazel does not cache any test results.

The manual adds that `yes` "may cache test failures and test runs with
`--runs_per_test`". `-t` and `-t-` are abbreviations for turning it on and off.

The implementation has two separate predicates:

- **`shouldAcceptCachedResult`**
  ([`TestRunnerAction.java#L626-L661`][tra-accept]) decides whether the **disk
  or remote** cache may be consulted. It returns false if the test is
  `external`, if the mode is `no`, or if the mode is `auto` and
  `runs_per_test > 1`. It deliberately ignores the previous result. If the
  predicate is false, `StandaloneTestStrategy` adds `no-cache` to the test
  spawn ([`StandaloneTestStrategy.java#L113-L119`][sts-nocache]).
- **`executeUnconditionally`**
  ([`TestRunnerAction.java#L541-L579`][tra-uncond]) decides whether the **local**
  action cache is bypassed. It returns true if `shouldAcceptCachedResult` is
  false. It also returns true if there is no readable previous
  `test.cache_status`, or if the previous result was marked non-cacheable. In
  `auto` mode only, it additionally returns true if the previous result did not
  pass: "otherwise we can get stuck forever in the event of a flaky failure".
  The local cache checker honours it before comparing digests
  ([`ActionCacheChecker.java#L548-L554`][acc-uncond]).

The effective matrix:

| Mode | Local action-cache reuse | Disk/remote lookup | Disk/remote store (passes only) |
|---|---|---|---|
| `auto` (default) | Only if the previous local verdict passed, the test is not `external`, and runs = 1 | Unless `external` or runs > 1 | Unless `external` or runs > 1 |
| `yes` | Any previous cacheable verdict, **including failures**, unless `external` | Unless `external` | Unless `external` |
| `no` | Never | Never | **Never** (the `no-cache` side effect, §3) |

A test also reruns in any mode when:
- its key or inputs changed (§1);
- the previous verdict is marked `cachable = false`. Infrastructure failures
  set this, as do a leftover `TEST_PREMATURE_EXIT_FILE` and a cancelled attempt
  ([`StandaloneTestStrategy.java#L650-L693`][sts-cachable], [`#L598-L613`][sts-cancel]);
- `test.cache_status` cannot be read, which prints "Cached test status was
  unexpectedly unavailable on disk" ([`TestRunnerAction.java#L663-L701`][tra-hit]);
- the remote-cache salt changed (§1.1).

**History.** Before Bazel 8.0.0, a previous local failure also disabled the
disk and remote lookup. A fixed test then could not hit the pass that was
already in the shared cache
([#11057](https://github.com/bazelbuild/bazel/issues/11057),
[#9389](https://github.com/bazelbuild/bazel/issues/9389)). Commit
[`e9709b7`](https://github.com/bazelbuild/bazel/commit/e9709b7d4b42aa3315d02bd658013508b2cb66f3)
split the logic into the two predicates above. It argues that busting the
*local* cache is enough to stop a flaky failure from sticking, "as a
disk/remote cache should never store failures". A regression test asserts the
fixed behaviour: pass, then fail, then pass again gives `(cached) PASSED` from
the disk cache ([`disk_cache_test.sh#L94-L129`][disk-test]).

---

## 3. Opt-outs

Execution requirements come from `tags`, but only for the tag prefixes listed
in `legalExecInfoKeys` ([`TargetUtils.java#L52-L62`][tu-legal]).
`TestTargetProperties` also turns `local`/`local = True` into `local` and
`exclusive` into `no-remote-exec`, the latter under the default
`--incompatible_exclusive_test_sandboxed`
([`TestTargetProperties.java#L76-L127`][ttp], [`TestConfiguration.java#L353-L364`][tc-excl]).
The cacheability predicates are in [`Spawns.java#L28-L56`][spawns]. The read
and write policies are in
[`RemoteExecutionService.java#L335-L359`][res-policy] and
[`Utils.java#L544-L549`][utils-upload].

| Opt-out | Local action cache | Disk cache | Remote cache read | Remote cache write | Remote exec | Doc |
|---|---|---|---|---|---|---|
| `external` tag | **bypassed** (always runs) | off | off | off | allowed | "will force test to be unconditionally executed (regardless of `--cache_test_results` value)" ([tags](https://bazel.build/reference/be/common-definitions#common.tags)); "disable test caching" ([tag conventions](https://bazel.build/reference/test-encyclopedia#tag-conventions)) |
| `--cache_test_results=no` / `--nocache_test_results` | bypassed | off | off | off | allowed | §2 |
| `no-cache` tag | **still applies** | off | off | off | allowed | "never being cached (locally or remotely) … Other caches, such as Skyframe or the persistent action cache, are not affected" ([tags](https://bazel.build/reference/be/common-definitions#common.tags)) |
| `local` tag / `local = True` | still applies | off | off | off | off (and unsandboxed) | [tags](https://bazel.build/reference/be/common-definitions#common.tags) |
| `no-remote-cache` tag | still applies | on | off | off | allowed | [tags](https://bazel.build/reference/be/common-definitions#common.tags) |
| `no-remote` tag | still applies | on | off | off | off | "equivalent to using both `no-remote-cache` and `no-remote-exec`" |
| `no-remote-cache-upload` tag | still applies | on | on | off | allowed | [`ExecutionRequirements.java#L320-L321`][er-upload] |
| `--noremote_accept_cached` | still applies | on | off | on | allowed (sent with `skip_cache_lookup`) | [`RemoteOptions.java#L258-L264`][ro-accept] |
| `--noremote_upload_local_results` | still applies | on | on | **off for local executions** | allowed | "Whether to upload locally executed action results to the remote cache" ([`RemoteOptions.java#L304-L312`][ro-upload]); used for a read-only cache ([docs](https://bazel.build/remote/caching#read-write-remote-cache)) |

The tags doc says that with a combined disk + remote cache, `no-remote-cache`
disables the disk part too, unless `--incompatible_remote_results_ignore_disk`
is set. That flag no longer exists in 9.2.0. The code allows the disk cache
whenever the spawn is not `no-cache`/`local`
([`RemoteExecutionService.java#L335-L359`][res-policy]), which is what the
table shows.

How these interact with tests:

- **`no-cache` does not mean "always run".** `executeUnconditionally` looks
  only at the mode, `external` and `runs_per_test`
  ([`TestRunnerAction.java#L541-L579`][tra-uncond]). A `no-cache` test that has not changed is
  still a local `(cached) PASSED`. Use `external` to force a rerun.
- **Rejecting cached results also stops publishing.** `--nocache_test_results`,
  `external` and `auto` with `runs_per_test > 1` all attach `no-cache` to the
  spawn. That disables reads *and* writes, and makes the REAPI `Action` carry
  `do_not_cache = true` ([`Utils.java#L444-L446`][utils-dnc]). The code
  admits this: "We want to reject a previously cached result, but not prevent
  the result of the current execution from being uploaded"
  ([`StandaloneTestStrategy.java#L113-L119`][sts-nocache]). The manual's
  statement that results are "*always* saved" is true of the local output tree
  only.
- **`--noremote_upload_local_results` only covers local executions.** A test
  that runs *on* a remote executor produces its `ActionResult` on the server,
  which may cache it (§5.2). The flag does not stop the disk cache either
  (`getWriteCachePolicy`, [`RemoteExecutionService.java#L347-L359`][res-write]).
- **`test.xml` generation is a separate spawn.** When a test does not write
  `XML_OUTPUT_FILE`, a second spawn generates `test.xml` from `test.log`. That
  spawn uses the target's execution info *without* the injected `no-cache`
  ([`StandaloneTestStrategy.java#L442-L482`][sts-xmlspawn]). Its result can
  therefore be served from cache even under `--nocache_test_results`; an
  integration test expects "1 remote cache hit" for it
  ([`remote_execution_test.sh#L2119-L2138`][re-nocache]).

---

## 4. Reporting a hit

**Status line.** `TestSummaryPrinter.getCacheMessage` prints `(cached) ` when
every shard/run of a target was cached, and `(k/n cached) ` when only some were
([`#L272-L284`][tsp]). "Cached" counts both local action-cache hits and
disk/remote hits: a result is counted as cached if
`result.isCached() || result.getData().getRemotelyCached()`
([`TestResultAggregator.java#L166-L184`][tra-agg]). The disk-cache regression
test expects the literal `(cached) PASSED` ([`disk_cache_test.sh#L126-L129`][disk-test-expect]).

**Summary counts.** A target counts as "executed" if at least one of its
shards/runs was neither kind of hit
([`AggregatingTestListener.java#L302-L305`][atl]). The final line is
`Executed %d out of %d tests: …`
([`TerminalTestResultNotifier.java#L313-L326`][trn-stats]). For example,
`Executed 0 out of 1 test: 1 test passes.` appears in
[#30576](https://github.com/bazelbuild/bazel/issues/30576). The
`short_uncached` and `detailed_uncached` summary formats hide cached passes
([`TerminalTestResultNotifier.java#L136-L141`][trn-hide1], [`#L166-L168`][trn-hide2]). In the
BEP, `TestResult.cached_locally` and `ExecutionInfo.cached_remotely` carry the
same distinction ([`build_event_stream.proto#L705-L706`][bep-local], [`#L751-L752`][bep-remote]).

**Where `test.log` and `test.xml` come from.**

- *Local action-cache hit.* Nothing runs and nothing is restored. `actionCacheHit`
  re-reads `test.cache_status` and maps whatever files are already in
  `bazel-testlogs/…` (`test.log`, `test.xml`, `test.outputs/…`). It then posts a
  `TestResult` with `cached = true`
  ([`TestRunnerAction.java#L663-L701`][tra-hit],
  [`StandaloneTestStrategy.java#L532-L540`][sts-cached]). With `--test_output`
  set to `errors` or `all`, the old log is re-printed
  ([`TerminalTestResultNotifier.java#L186-L196`][trn-cachedout]). Under
  Build-without-the-Bytes, those files may not exist locally, and the code
  notes that the mapping then misreports them
  ([`TestRunnerAction.java#L407-L416`][tra-bwob]).
- *Disk or remote hit.* The action runs, but its test spawn is served from
  the cache. Declared spawn outputs are downloaded, subject to
  `--remote_download_*`. `test.xml` is a declared output in Bazel "so that it
  behaves properly with Build without the Bytes"
  ([`TestActionBuilder.java#L62-L66`][tab-xml]). The cached stdout and stderr
  are "always" downloaded into the spawn's `FileOutErr`, which for a test is
  `test.log` ([`RemoteExecutionService.java#L1339-L1348`][res-outerr]). The
  verdict is recorded as `remotely_cached` and written back into the local
  `test.cache_status` ([`StandaloneTestStrategy.java#L362-L382`][sts-exinfo]).
  If the cached result has no `test.xml`, the generation spawn from §3 runs, or
  is itself served from cache ([`#L839-L871`][sts-xml]).

---

## 5. Failures in the cache

### 5.1 What Bazel does

- **Local tier: failures are stored.** Every verdict is persisted to
  `test.cache_status`. A failed attempt is marked `cachable` exactly when its
  spawn status "is considered a user error"
  ([`StandaloneTestStrategy.java#L666-L671`][sts-usererr]). That covers
  `NON_ZERO_EXIT`, `TIMEOUT`, `OUT_OF_MEMORY` and `EXECUTION_DENIED`, but not
  `EXECUTION_FAILED` or `REMOTE_CACHE_FAILED`
  ([`SpawnResult.java#L43-L106`][spawn-status]). With the default
  `--test_keep_going`, a failing test does not fail the action, so the local
  entry is written. `auto` never reuses a failing verdict; `yes` does (§2).
- **Disk and remote tiers: Bazel never writes failures.** The store path runs
  only if `status == SUCCESS && exitCode == 0`
  ([`RemoteExecutionService.java#L1533-L1550`][res-commit],
  [`RemoteSpawnCache.java#L268-L275`][rsc-store];
  [`RemoteSpawnRunner.java#L679-L687`][rsr-upload] for remote-execution
  fallback). A regression test checks that a failing test's `test.log` and
  `test.xml` are not uploaded
  ([#7232](https://github.com/bazelbuild/bazel/issues/7232),
  [`remote_execution_test.sh#L613-L636`][re-failtest]).
- **Disk and remote tiers: Bazel never *serves* failures either.**
  - Spawn-cache lookup: "In case the remote cache returned a failed action
    (exit code != 0) … we treat it as a cache miss"
    ([`RemoteSpawnCache.java#L148-L152`][rsc-miss]).
  - Remote execution: a cached lookup result with a non-zero exit is ignored,
    "mostly in order to avoid caching flaky actions (tests)"
    ([`RemoteSpawnRunner.java#L214-L223`][rsr-miss]). If `Execute` returns
    `cached_result = true` with a failure, Bazel retries with
    `skip_cache_lookup = true` ([`#L303-L314`][rsr-retry];
    [`RemoteExecutionService.java#L1929`][res-skip]).
  - A failure that was *freshly executed* by the remote executor is accepted
    as the result of this run.

### 5.2 What the Remote Execution API says

- A server "MAY choose to cache the result in the ActionCache unless
  `do_not_cache` is `true`. Clients SHOULD expect the server to do so."
  ([proto `#L656-L673`][reapi-action])
- `do_not_cache`: "the `Action`'s result cannot be cached, and in-flight
  requests for the same `Action` may not be merged."
  ([`#L717-L719`][reapi-dnc]) Bazel sets it for any spawn that may not be
  cached remotely ([`RemoteExecutionService.java#L560`][res-dnc]).
- `ExecuteResponse.status`: "If the status code is other than `OK`, then the
  result MUST NOT be cached." ([`#L1669-L1678`][reapi-status]) A timeout, for
  example, arrives as `DEADLINE_EXCEEDED`.
- `ActionResult.exit_code` is documented only as "The exit code of the
  command." ([`#L1386-L1387`][reapi-exit]) **The spec has no rule about caching
  non-zero exits.** An action that completed with `OK` status and exit code 1
  is a valid, cacheable `ActionResult`. `UpdateActionResult` also lets any
  client write one, and "Server implementations MAY modify" it
  ([`#L176-L195`][reapi-ac]).
- `skip_cache_lookup` forces execution. Results from such executions "are
  still eligible to be entered into the action cache … and services SHOULD
  overwrite any existing entries", which is how a client replaces a "poisoned"
  entry ([`#L1592-L1605`][reapi-skip]).

So whether failures are cached server-side is a **server policy** that the spec
leaves open. Bazel protects itself on the client, and says so:
"this can only occur with a remote execution implementation that caches
failures, as we never upload them to a disk/remote cache ourselves"
([`TestRunnerAction.java#L626-L643`][tra-accept-doc]).

### 5.3 `flaky = True`, `--flaky_test_attempts` and caching

- Retries happen **inside a single `TestRunnerAction`**
  ([`TestRunnerAction.java#L1216-L1280`][tra-attempts]). The default is one
  attempt, or three for `flaky = True`. `--flaky_test_attempts` overrides this
  per label, up to a maximum of 10
  ([`TestStrategy.java#L272-L307`][ts-attempts];
  [user manual](https://bazel.build/docs/user-manual#flaky-test-attempts);
  [`flaky` attribute](https://bazel.build/reference/be/common-definitions#test.flaky)).
- A retry reuses **the same spawn** (`getFlakyRetryRunner` returns `this`,
  [`TestActionContext.java#L210-L218`][tac-flaky]). Each attempt goes through
  the spawn cache. Failed attempts are not stored; a passing attempt is stored
  as an ordinary exit-0 result (§5.1).
- If an attempt passes after earlier failures, the action's verdict becomes
  `FLAKY` ([`StandaloneTestStrategy.java#L218-L229`][sts-flaky-status]), with
  `test_passed = true`, and is persisted locally. `auto` therefore reuses it
  locally, and it counts as passed for the exit code
  ([user manual](https://bazel.build/docs/user-manual#flaky-test-attempts)).
- *(inferred)* The `ActionResult` in the shared cache describes only the
  passing attempt. A later disk or remote hit, on another machine or after
  `bazel clean`, reports `(cached) PASSED`, so the flakiness is not visible
  there.
- Neither `flaky` nor `--flaky_test_attempts` is in the key (§1.2). Raising or
  lowering the number of attempts never invalidates a cached result.
- `--runs_per_test > 1` produces separate actions per run, with `run_N_of_M`
  directories and `TEST_RUN_NUMBER`. Under `auto` it disables caching
  entirely. `--runs_per_test_detects_flakes` turns mixed pass/fail runs of a
  shard into `FLAKY` ([`TestConfiguration.java#L260-L296`][tc-runs];
  [user manual](https://bazel.build/docs/user-manual#runs-per-test)).

---

## 6. Pitfalls

1. **Non-hermetic tests give stale hits.** The test encyclopedia requires that
   a test's outcome "must depend only on" declared sources, declared build
   products, and "resources whose behavior is guaranteed by the test runner to
   remain constant". It then admits "such behavior is not enforced"
   ([purpose of tests](https://bazel.build/reference/test-encyclopedia#purpose-of-tests)).
   Host tools are not tracked either ([known issues](https://bazel.build/remote/caching#known-issues),
   [#4558](https://github.com/bazelbuild/bazel/issues/4558)). Tests that talk
   to servers, read the clock, or shell out to `/usr/bin/…` are reused as long
   as their declared inputs are unchanged. The only documented remedy is the
   `external` tag.
2. **`no-cache` is not "always run"** (§3). The docs say it leaves "the
   persistent action cache" alone. Users who tag a test `no-cache` expecting a
   rerun still get a local `(cached) PASSED`.
3. **`--test_timeout` did not bust the local cache** before master
   [`a05f041`](https://github.com/bazelbuild/bazel/commit/a05f041e4714a324d23d731641674deeac9e2744).
   A test that passed in 2 s under `--test_timeout=10` was reported as
   `(cached) PASSED` under `--test_timeout=1`, while the disk cache correctly
   missed. The reporter traced the bug back to 2015
   ([#30576](https://github.com/bazelbuild/bazel/issues/30576)). This is the
   concrete risk behind rule 2: two tiers, two key definitions.
4. **`--test_env` and `--action_env` leakage.**
   - Inherited variables put the client's *value* into the key (§1.2). User-
     or machine-specific values such as `HOME`, `USER` or a personal `PATH`
     therefore split the shared cache per machine.
   - The strict-env flag warns that inheriting "can prevent cross-user caching
     if a shared cache is used"
     ([`BazelRuleClassProvider.java#L77-L86`][brcp-help]). The caching docs
     list "Environment variables leaking into an action"
     ([known issues](https://bazel.build/remote/caching#known-issues)).
   - Until 9.0.0, changing `--test_env` also discarded the analysis cache
     ([#7450](https://github.com/bazelbuild/bazel/issues/7450), fixed by
     [#24398](https://github.com/bazelbuild/bazel/pull/24398) /
     [`43ebb95`](https://github.com/bazelbuild/bazel/commit/43ebb95d1b1a66e7d46dc5e573309ace68f471a8)).
     That is a speed problem, not a correctness one.
5. **`$HOME`.**
   - Before 9.0.0, tests ran "without `HOME` set", because `test-setup.sh`
     assigned it without exporting it
     ([#10652](https://github.com/bazelbuild/bazel/issues/10652)). Whatever a
     test or tool then read from the real home directory was outside the key.
   - Since [`03b0045`](https://github.com/bazelbuild/bazel/commit/03b00453bbf58a30e9c878b75196f58d1dd9a4d3)
     (9.0.0), `HOME=$TEST_TMPDIR` is exported, as the encyclopedia recommends
     ([initial conditions](https://bazel.build/reference/test-encyclopedia#initial-conditions)).
     The encyclopedia also says tests "must not attempt to write to" the user's
     home directory ([users and groups](https://bazel.build/reference/test-encyclopedia#users-groups)).
   - The workaround `--test_env=HOME` re-introduces the leak, and also makes
     the key per-user (pitfall 4). `USER` is still filled in from `whoami`
     inside `test-setup.sh`, outside the key ([`test-setup.sh#L70-L73`][setup-user]).
6. **Sticky failures (historical).** Before 8.0.0, a local failure disabled
   shared-cache lookups until the test was rerun and passed
   ([#11057](https://github.com/bazelbuild/bazel/issues/11057),
   [#9389](https://github.com/bazelbuild/bazel/issues/9389)); see §2.
7. **The local tier remembers one result per output path.** Moving between two
   source states (branch A, branch B, branch A) always misses locally. It can
   only hit the disk or remote cache, which is exactly the [#11057](https://github.com/bazelbuild/bazel/issues/11057) scenario
   ([`ActionCacheUtils.java#L39-L47`][acu]).
8. **The manual's "always saved" is local only.** `--nocache_test_results`
   (and `external`) suppress uploads as well (§3).
9. **Flaky passes mask flakiness downstream** *(inferred)*; see §5.3.

---

## Appendix: permalink index

All Bazel links are pinned to the 9.2.0 release commit `8220c61` unless marked
master.

[tab-loop]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestActionBuilder.java#L344-L443
[tab-inputs]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestActionBuilder.java#L207-L233
[tab-xml]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestActionBuilder.java#L62-L66
[tra-key]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L507-L537
[tra-uncond]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L541-L579
[tra-accept]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L626-L661
[tra-hit]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L663-L701
[tra-save]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L586-L597
[tra-clientenv]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L163-L169
[tra-filter]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L754-L757
[tra-seed]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L738-L751
[tra-guid]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L110
[tra-bwob]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L407-L416
[tra-attempts]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L1216-L1280
[master-timeout-line]: https://github.com/bazelbuild/bazel/blob/151450cd52144e37a07ba2193763d8812ddbe12b/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L538
[tes-args]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestTargetExecutionSettings.java#L71-L73
[ttp]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestTargetProperties.java#L76-L127
[ts-attempts]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestStrategy.java#L272-L307
[ts-post]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestStrategy.java#L309-L316
[ts-tmpname]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestStrategy.java#L336-L343
[tac-flaky]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestActionContext.java#L210-L218
[tc-cache]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestConfiguration.java#L168-L186
[tc-expire]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestConfiguration.java#L187-L194
[tc-runs]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestConfiguration.java#L260-L296
[tc-excl]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestConfiguration.java#L353-L364
[eo-flaky]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/ExecutionOptions.java#L239-L260
[sts-env]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L76-L88
[sts-spawn]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L98-L145
[sts-exinfo]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L362-L382
[sts-xmlspawn]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L442-L482
[sts-cached]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L532-L540
[sts-cancel]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L598-L613
[sts-xml]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L839-L871
[tp]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/TestPolicy.java#L60-L102
[asps]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/AbstractSpawnStrategy.java#L119-L167
[akc]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionKeyComputer.java#L36-L57
[acc-must]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionCacheChecker.java#L533-L592
[acc-env]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionCacheChecker.java#L291-L300
[acc-cachekey]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionCacheChecker.java#L876
[acu]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionCacheUtils.java#L39-L47
[ae-addto]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionEnvironment.java#L134-L137
[rav]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/RunfilesArtifactValue.java#L87-L117
[spawns]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/Spawns.java#L28-L56
[spawn-status]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/SpawnResult.java#L43-L106
[tu-legal]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/packages/TargetUtils.java#L52-L62
[res-policy]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L335-L359
[res-salt]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L370-L386
[res-build]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L504-L584
[res-outerr]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L1339-L1348
[res-commit]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L1533-L1550
[res-skip]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L1929
[rsc-miss]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteSpawnCache.java#L148-L152
[rsc-store]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteSpawnCache.java#L268-L275
[rsr-miss]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteSpawnRunner.java#L214-L223
[rsr-retry]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteSpawnRunner.java#L303-L314
[rsr-upload]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteSpawnRunner.java#L679-L687
[utils-action]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/util/Utils.java#L431-L454
[utils-upload]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/util/Utils.java#L544-L549
[ro-accept]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/options/RemoteOptions.java#L258-L264
[ro-upload]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/options/RemoteOptions.java#L304-L312
[rm-salt]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteModule.java#L1200-L1222
[tsp]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TestSummaryPrinter.java#L272-L284
[tra-agg]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TestResultAggregator.java#L166-L184
[atl]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/AggregatingTestListener.java#L302-L305
[trn-stats]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TerminalTestResultNotifier.java#L313-L326
[trn-cachedout]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TerminalTestResultNotifier.java#L186-L196
[brcp-strict]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/bazel/rules/BazelRuleClassProvider.java#L72-L87
[setup]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/tools/test/test-setup.sh#L64-L73
[disk-test]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/test/shell/bazel/disk_cache_test.sh#L94-L129
[re-failtest]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/test/shell/bazel/remote/remote_execution_test.sh#L613-L636
[re-nocache]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/test/shell/bazel/remote/remote_execution_test.sh#L2119-L2138
[reapi-ac]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L176-L195
[reapi-action]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L656-L673
[reapi-timeout]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L686-L715
[reapi-dnc]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L717-L719
[reapi-exit]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1386-L1387
[reapi-skip]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1592-L1605
[reapi-status]: https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1669-L1678
[sts-timeout]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L120-L121
[sts-nocache]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L113-L119
[sts-cachable]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L650-L693
[sts-usererr]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L666-L671
[sts-flaky-status]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/StandaloneTestStrategy.java#L218-L229
[tp-order]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/exec/TestPolicy.java#L88-L95
[akc-plat]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionKeyComputer.java#L48-L56
[acc-uncond]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ActionCacheChecker.java#L548-L554
[er-upload]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/actions/ExecutionRequirements.java#L320-L321
[utils-dnc]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/util/Utils.java#L444-L446
[res-write]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L347-L359
[res-dnc]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/remote/RemoteExecutionService.java#L560
[tra-accept-doc]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/analysis/test/TestRunnerAction.java#L626-L643
[trn-hide1]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TerminalTestResultNotifier.java#L136-L141
[trn-hide2]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/runtime/TerminalTestResultNotifier.java#L166-L168
[bep-local]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/buildeventstream/proto/build_event_stream.proto#L705-L706
[bep-remote]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/buildeventstream/proto/build_event_stream.proto#L751-L752
[brcp-help]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/main/java/com/google/devtools/build/lib/bazel/rules/BazelRuleClassProvider.java#L77-L86
[setup-user]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/tools/test/test-setup.sh#L70-L73
[disk-test-expect]: https://github.com/bazelbuild/bazel/blob/8220c6198837d5c13d53fea211cf3282aa12408a/src/test/shell/bazel/disk_cache_test.sh#L126-L129
