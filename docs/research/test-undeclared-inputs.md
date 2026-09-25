# What do turnkey's test targets read beyond their declared inputs?

Research for Beadwork ticket `turnkey-w55.3`. It feeds the design of **test result caching**: skipping a test whose inputs match those of a recorded run and reporting the recorded result instead. A cached result is sound only if everything the test reads is in its key. It is reusable only if the key holds nothing specific to one checkout, because hits must work across jj workspaces and git worktrees of the same revision on the same machine.

This is static analysis only. Nothing was built or run.

**The key.** Throughout, "the key" means what buck2 already hashes for a test's execution request. That is:

- the args
- the *declared* env
- the project-relative working directory
- the input-root digest
- the outputs, the timeout and the platform

Source: `buck2:app/buck2_execute/src/execute/command_executor.rs:191-258`. Anything outside that set is "not in the key" below.

**Severity.**

- **Wrong hit:** the test reads something that is not in the key, so a stale recorded pass could be served.
- **Spurious miss:** the key holds a checkout-specific value, so another checkout of the same revision never reuses the result.

The word in brackets is likelihood:

- **certain:** happens on every cross-checkout lookup.
- **medium** / **low:** plausible in normal use.
- **negligible:** would need an odd host state.
- **latent:** the rule allows it, but no current test does it.

## 1. Per-target table

The hazard IDs (G1, P1, S1, …) are defined in §4. Three cross-cutting hazards apply to every row and are not repeated in it: X1, X2 and X3 (§3).

| Target | Rule | Hazards | Severity |
|---|---|---|---|
| `//src/go/pkg/buckgen:buckgen_test` | go_test | G1: absolute test-binary path in the command | Spurious miss (certain) |
| `//src/go/pkg/cellfresh:cellfresh_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/extraction:extraction_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/godeps:godeps_test` | go_test | G1. G2: the fixtures (`//src/testdata:godeps_fixtures`) are not declared, so the 3 `TestIntegration_*` cases always `t.Skip` under buck2 and report a pass | Spurious miss (certain). Wrong hit (latent): it becomes real if the skip is fixed without declaring `resources` |
| `//src/go/pkg/goparse:goparse_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/localconfig:localconfig_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/mapper:mapper_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/prefetchcache:prefetchcache_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/rules:rules_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/staleness:staleness_test` | go_test | G1. G3: reads `$HOME`. G4: file age is ordered by a 10 ms sleep | Spurious miss (certain). Wrong hit (negligible). G4 is a flake that a cache would freeze (low) |
| `//src/go/pkg/starlark:starlark_test` | go_test | G1 | Spurious miss (certain) |
| `//src/go/pkg/syncconfig:syncconfig_test` | go_test | G1. G5: stats the host path `/nonexistent/.turnkey/sync.toml` | Spurious miss (certain). Wrong hit (negligible) |
| `//src/python/cargo:test_toml` | python_test | P1: interpreter taken from PATH. P2: absolute `.pex` path and `PYTHON_SOURCE_MAP`. P3: host and user site-packages. P6: walks the ancestors of `$TMPDIR` looking for `Cargo.toml` | Spurious miss (certain). Wrong hit (medium: P1; latent: P3; negligible: P6) |
| `//src/python/cargo:test_features` | python_test | P1, P2, P3. P6: reads the host path `/nonexistent/path.toml` | Spurious miss (certain). Wrong hit (medium: P1; latent: P3; negligible: P6) |
| `//src/python/cfg:test` | python_test | P1, P2, P3 | Spurious miss (certain). Wrong hit (medium: P1; latent: P3) |
| `//src/examples/python-hello:python-hello-test` | python_test | P1, P2, P3 | Spurious miss (certain). Wrong hit (medium: P1; latent: P3) |
| `//e2e/fixtures/multi-language/rust_lib:greeting-test` | rust_test | None found. The target is live in turnkey's root cell because the generated `.buckconfig` has no `[project] ignore` | None |
| `//src/cmd/check-rust-edition-rs:check-rust-edition-rs-test` | rust_test | None found. The crate has zero `#[test]` functions, so the pass is vacuous | None |
| `//src/cmd/check-source-coverage-rs:check-source-coverage-rs-test` | rust_test | None found. Zero `#[test]` functions (vacuous pass) | None |
| `//src/rust/starlark-parse:starlark-parse-test` | rust_test | None found | None |
| `//src/rust/prefetch-cache:prefetch-cache-test` | rust_test | None found | None |
| `//src/rust/composition:composition-test` | rust_test | R1: reads host `/tmp` | Wrong hit (negligible) |
| `//src/rust/composition:integration-tests` | rust_test | None found. The FUSE tests are compiled out, which is a coverage gap rather than a caching hazard | None |
| `//src/examples/rust-hello:rust-hello-test` | rust_test | None found | None |
| `//src/examples/solidity-hello:counter_test` | solidity_test | S1: solc is chosen by forge's auto-detect under `$HOME`, using the network if the version is missing, and the Nix solc is not used. S2: `~/.foundry/foundry.toml` is merged if present. S4: fuzzing is unseeded. S5: host tools come from PATH. (S3 is read here too, with no effect) | Wrong hit (medium: S1, S4; latent: S2; low: S5) |
| `//src/examples/solidity-hello-deps:token_test` | solidity_test | S1, S2, S4, S5. S3: the OpenZeppelin sources are read through the literal path `.turnkey/soldeps` and are not declared on the test action | Wrong hit (medium: S1, S4; low: S3, S5; latent: S2) |
| `//src/examples/jsonnet-config:common-test` | jsonnet_test | J1: undeclared imports resolve in place, and `-J .` points at the project root. J2: `bash` comes from PATH | Wrong hit (latent: J1; low: J2) |
| `//src/examples/jsonnet-config:config-dev-test` | jsonnet_test | J1. J2: `bash`, `mktemp` and `diff` come from PATH | Wrong hit (latent: J1; low: J2) |

The targets were enumerated with `grep -rn --include=rules.star '_test('`, giving 28 targets. `src/cmd/tk/isolation_dir_test.go` has no `go_test` target. It is swept into the `go_binary` glob in `src/cmd/tk/rules.star` instead, so no test runs it.

## 2. Key findings

1. **The key holds absolute paths for 16 of the 28 targets (Go and Python).**
   - The prelude sets `use_project_relative_paths = run_from_project_root = re_executor != None` (`prelude/go/go_test.bzl:163-165`, `prelude/python/compute_providers.bzl:56-57`).
   - Turnkey has no RE profile, so both flags are `False`. buck2 then renders every artifact in `command` and `env` as an absolute path (`buck2:app/buck2_test/src/orchestrator.rs:1493-1501`).
   - Result: a cross-checkout hit is impossible for every Go and Python test.
   - Rust hard-codes both flags to `True`. The two turnkey rules leave them unset, which defaults to `True` in OSS buck2. So Rust, Solidity and Jsonnet keys are clean.
2. **The test environment is cleared, then rebuilt from an allowlist.**
   - Only `PATH`, `USER`, `LOGNAME`, `HOME`, `TMPDIR` and `XDG_RUNTIME_DIR` survive. They are snapshotted once per daemon (`buck2:app/buck2_execute/src/execute/environment_inheritance.rs:19-28, 77-103`), and none of them is in the key.
   - So `PATH` and `HOME` are the only ambient-env channels a test can read.
   - Other variables are invisible to tests, which rules out several apparent hazards: `FOUNDRY_*`, `JSONNET_PATH`, `BUCK_PROJECT_ROOT`, `PYTHONPYCACHEPREFIX`, `RUST_TEST_THREADS`.
3. **PATH carries real inputs that are not in the key.**
   - The Python interpreter is the bare string `python3` (P1).
   - The solidity and jsonnet test scripts are `#!/usr/bin/env bash` and call `mktemp`, `diff`, `cp`, `realpath` and similar from PATH (S5, J2).
   - Bumping `python-toolchain` changes neither the command nor any input digest.
4. **`solidity_test` never uses the toolchain's Nix `solc`.**
   - The generated `foundry.toml` leaves foundry's default `auto_detect_solc = true` in place. forge then picks a solc through svm under `$HOME` (`~/Library/Application Support/svm` on this machine, which holds 0.8.15–0.8.30), downloading one if needed.
   - It also merges `~/.foundry/foundry.toml` if that file exists.
   - Fuzz tests run with a "stochastic fuzzer" because no seed is set.
   - These are the largest wrong-hit risks in the repo (S1, S2, S4).
5. **The soldeps cell is passed as a string.** `solidity_test` passes `read_root_config("cells", "soldeps")`, which is the literal `.turnkey/soldeps`, not an artifact.
   - Cross-checkout reuse is fine, because the unresolved relative string is what enters the key.
   - The cell's content, meaning `remappings.txt` and the OpenZeppelin sources for `token_test`, is not in the test's key (S3).
6. **There is no sandbox.** Sources, and the Nix cells behind `.turnkey/*`, are read in place (`buck2:app/buck2_artifact/src/artifact/artifact_type.rs:243-253`).
   - Only rules that stage their inputs are shielded: Go's test main `chdir`s into buck-out, and forge runs in a `mktemp` directory.
   - Jsonnet resolves undeclared imports straight from the checkout, and adds `-J .`, which is the project root (J1).
7. **A running daemon does not see cell symlinks being retargeted** (`src/go/pkg/cellfresh/cellfresh.go:1-9`). `tk` kills the daemon when it detects this (`src/cmd/tk/main.go:278`); raw `buck2` does not. Any cache that takes digests from buck2 is therefore only sound when driven through `tk` (X3).
8. **Some passes are vacuous.**
   - `godeps_test`'s integration cases skip because their fixtures are never declared as `resources` (G2).
   - The two `check-*-rs-test` targets contain no tests.
   - The hazard is not caching as such: a cached pass here certifies nothing.
9. **buck2's native caching cannot fire under turnkey.** buck2 has `ExternalRunnerTestInfo.supports_test_execution_caching`, but turnkey's local-only executor has a no-op action cache (`buck2:app/buck2_server/src/daemon/common.rs:258-273`). The gap in the glossary definition is real.

## 3. How buck2 runs a turnkey test (applies to every rule)

### 3.1 The key and what sits outside it

- **The request digest** covers:
  - `request.all_args_vec()`
  - `request.env()` (declared env only)
  - the project-relative `working_directory`
  - the input-directory fingerprint, output paths, timeout and platform

  Source: `buck2:app/buck2_execute/src/execute/command_executor.rs:191-258`.
- **Not in the digest but present at runtime:**
  - The allowlisted inherited env (§3.3).
  - `PWD`, set to the absolute cwd (`buck2:app/buck2_execute_impl/src/executors/local.rs:1596-1619`).
  - `BUCK2_DAEMON_UUID` and `BUCK_BUILD_ID` (`local.rs:664-666`).
  - The executable path made absolute at spawn (`buck2:app/buck2_execute_local/src/lib.rs:415-427`).

  The last three are checkout- or run-specific, so they must stay out of any key. No turnkey test reads them.
- **Runner arguments land in the request.** The OSS runner appends `--test-arg` and `--env` runner arguments to the request verbatim (`buck2:app/buck2_test_runner/src/runner.rs:135-183`). They are therefore in the digest (see §5 on `.turnkey/local.toml`).

### 3.2 Working directory and path rendering

- **cwd.** If `run_from_project_root` is true (or forced), cwd is the project root. Otherwise it is the target's cell root (`buck2:app/buck2_test/src/orchestrator.rs:1460-1470`). Turnkey's generated buckconfig maps `root = .` (`nix/devenv/turnkey/buck2.nix:344-349`), so both resolve to the project root.
- **Paths.** If `use_project_relative_paths` is true, artifacts render project-relative through `DefaultCommandLineContext`. Otherwise they render absolute through `AbsCommandLineContext` (`orchestrator.rs:1493-1501`).
- **Defaults when a flag is unset:**
  - `use_project_relative_paths` defaults to `is_open_source()`, which is `true` in OSS builds (`buck2:app/buck2_build_api/src/interpreter/rule_defs/provider/builtin/external_runner_test_info.rs:145-151`; `buck2:app/buck2_core/src/lib.rs:84-86`).
  - `run_from_project_root` defaults to `true` (`external_runner_test_info.rs:153-159`).
- **Forcing both flags.** `buck2 test --unstable-allow-all-tests-on-re` forces both to true (`buck2:app/buck2_client/src/commands/test.rs:115-118, 343-348`).
- **Where outputs live.** buck-out is `<project_root>/buck-out/<isolation_dir>` (`buck2:app/buck2_common/src/invocation_paths.rs:36-38, 115-125`). With `BUCK_ISOLATION_DIR=.turnkey` (`nix/devenv/turnkey/buck2.nix:1037`), a project-relative artifact is `buck-out/.turnkey/…`, which is identical across checkouts.

| Rule | `run_from_project_root` | `use_project_relative_paths` | Artifact paths in key | Source |
|---|---|---|---|---|
| go_test | `False` (no RE executor) | `False` | **absolute** | `prelude/go/go_test.bzl:163-165` |
| python_test | `False` | `False` | **absolute** | `prelude/python/compute_providers.bzl:56-57` |
| rust_test | `True` | `True` | project-relative | `prelude/rust/rust_binary.bzl:553-554` |
| solidity_test | unset → `True` | unset → `True` | project-relative | `nix/buck2/prelude-extensions/solidity/solidity_test.bzl:199-202` |
| jsonnet_test | unset → `True` | unset → `True` | project-relative | `nix/buck2/prelude-extensions/jsonnet/jsonnet_test.bzl:142-145` |

**Why go and python get `False`:** `re_executor` is always `None` in turnkey. `get_re_executors_from_props` returns `(None, {})` when there are no RE props (`prelude/tests/re_utils.bzl:74-82`). Props come only from a target's `remote_execution` attribute, which no turnkey test sets, or from the toolchain's `default_profile` (`re_utils.bzl:19-52`). Turnkey declares `remote_test_execution_toolchain` without attributes (`nix/buck2/mappings.nix:126-140`), so `default_profile = None` (`prelude/toolchains/remote_test_execution.bzl:14, 33`). The execution platform is local-only (`prelude/platforms/defs.bzl:21-25`).

### 3.3 Environment (X1)

- **The allowlist.** Tests run with `EnvironmentInheritance::test_allowlist()` (`buck2:app/buck2_test/src/orchestrator.rs:1573`). It is `clear: true` plus `PATH, USER, LOGNAME, HOME, TMPDIR, XDG_RUNTIME_DIR` on unix OSS builds (`buck2:app/buck2_execute/src/execute/environment_inheritance.rs:19-28, 77-103`).
- **When the values are captured.** They are read from the daemon's own environment once, through a `OnceLock` (`environment_inheritance.rs:84-96`). The daemon inherits the environment of the client that spawned it.
- **One daemon per checkout.** The daemon directory is `$HOME/.buck/buckd/<absolute project root>/<isolation dir>` (`buck2:app/buck2_common/src/invocation_paths.rs:56-84`). Each checkout therefore has its own daemon and its own env snapshot, and a `direnv` reload does not reach an already-running daemon.
- **No scratch dir for tests.** `TMPDIR` is overridden only when the request carries a scratch path (`local.rs:623-648`). Test requests do not carry one, so tests see the host `$TMPDIR`.
- **Hazard.** None of the six inherited variables is in the key. The rules below read `PATH` (P1, S5, J2) and `HOME` (S1, S2, P3, G3). Severity is **wrong hit**. Keying on the raw values instead would be a **spurious miss** whenever they hold checkout-specific entries. Turnkey itself adds none to `PATH`; the one `$PWD`-derived variable it exports, `PYTHONPYCACHEPREFIX` (`nix/devenv/turnkey/default.nix:58-62`), is not on the allowlist.

### 3.4 No sandbox (X2)

- **Sources are never materialized.** A source artifact needs materialization only if its cell is an `[external_cells]` cell (`buck2:app/buck2_artifact/src/artifact/artifact_type.rs:243-253`). Turnkey's Nix cells are plain `[cells]` entries (`nix/devenv/turnkey/buck2.nix:344-349`), so they are read in place through the `.turnkey/*` symlinks, like repo sources.
- **What a test can reach.** On macOS nothing stops a test from reading any checkout file, anything under `$HOME`, or the network.
- **How each rule is exposed:**
  - **Protected** by staging:
    - Go: the test main `chdir`s into buck-out (§4.1).
    - Solidity: forge runs in a `mktemp -d` directory (§4.4).
  - **Unprotected** (cwd is the checkout root): Rust, Python and Jsonnet. None of the current Rust or Python tests read checkout files through relative paths.

### 3.5 The `.turnkey/*` cells: which form enters the key (X3)

| Consumer | Form in the key | Cross-checkout | Content tracked |
|---|---|---|---|
| Source artifacts from `godeps//`, `rustdeps//`, `pydeps//` and `soldeps//` targets (compile inputs, `solidity_library` deps) | project-relative path `.turnkey/<cell>/…` plus a content digest of the resolved file | yes | yes, but only if the daemon has been restarted since the symlink was retargeted (below) |
| `solidity_test --soldeps-cell` | the literal string `.turnkey/soldeps` (`solidity_test.bzl:188-192`) | yes | **no** (S3) |
| Toolchain binaries: `forge` (`nix/buck2/mappings.nix:237-238`), `jrsonnet` (`mappings.nix:459`) | resolved `/nix/store/…` strings | yes | yes (the store path changes with the content) |
| Toolchains `go`, `rustc`, `python3` (`prelude/toolchains/go/system_go_toolchain.bzl:38`, `prelude/toolchains/rust.bzl:45`, `prelude/toolchains/python.bzl:44-47`) | bare names, resolved through `PATH` at run time | yes | **no** (P1; for go and rustc only through the output binaries) |
| Python link tree entries pointing into `pydeps//` | a `../…/nix/store/…` symlink whose target text depends on how deep the checkout sits (`prelude/python/tools/make_py_package_modules.py:244-254`) | **no** (P4) | yes |

**Daemon freshness.** `cellfresh` documents that "Buck2's daemon doesn't detect" a retargeted cell symlink and "caches the resolved cell contents from the old path" (`src/go/pkg/cellfresh/cellfresh.go:1-9`). `tk` runs `cellfresh.Check` before every command and kills the daemon on change (`src/cmd/tk/main.go:278`; `cellfresh.go:185-196`). A plain `buck2 test` does not. It would compute digests, and therefore keys, from the old cell contents, while the test itself may read the new ones through the symlink. Severity is **wrong hit** unless caching only ever runs under `tk`.

### 3.6 buck2's own test caching

`ExternalRunnerTestInfo` has `supports_test_execution_caching`, which defaults to `false` (`external_runner_test_info.rs:203-209`). The orchestrator consults the action cache only when it is set (`orchestrator.rs:399, 1122-1124`). Even then, turnkey's `Executor::Local` has `action_cache_checker = NoOpCommandOptionalExecutor` (`buck2:app/buck2_server/src/daemon/common.rs:258-273`). Every `buck2 test` therefore re-executes. Test results are also not memoized in DICE.

## 4. Per-rule details

In the citations, `prelude/…` means the pinned prelude at `.turnkey/prelude/…`. Turnkey's own extensions are cited at their source in `nix/buck2/prelude-extensions/…`. The copy in the pinned prelude is byte-identical (checked with `diff -r` against the main checkout's `.turnkey/prelude`).

### 4.1 `go_test` (12 targets)

**Declared inputs and `ExternalRunnerTestInfo`**

- **`resources`** are copied next to the test binary with `copy_file(resource.short_path, …)` and added as hidden inputs of the command (`prelude/go/go_test.bzl:143-148`).
- **`command`** is `[cmd_args(bin, hidden = runtime_files + external_debug_info + copied_resources)]`.
- **`env`** is `ctx.attrs.env` (`go_test.bzl:153-166`). There is no `run_env`. `inject_test_run_info` only rewrites the `RunInfo` used by `buck2 run` (`prelude/test/inject_test_run_info.bzl:25-40`).
- **Path flags:** `run_from_project_root` and `use_project_relative_paths` are both `re_executor != None`, which is `False` here. The source marks this `# FIXME: Consider setting to true` (`go_test.bzl:163-165`).
- **Usage in turnkey:** none of the 12 `rules.star` stanzas sets `resources`, `env` or `remote_execution`.

**Runtime behaviour relevant to undeclared reads**

- **The test main changes directory.** The generated test main `chdir`s to the directory of the test binary before running tests (`prelude/go/tools/testmaingen.go:428-443`). Relative reads such as `testdata/…` or `../../x` therefore resolve inside `buck-out/.turnkey/…/__<name>__/`, where only declared `resources` exist. An undeclared relative read fails; it does not silently read the checkout.
- **Compiled-in paths are project-relative.** Go compilation runs with `-trimpath %cwd%` (`prelude/go/package_builder.bzl:400, 452, 492`), and `%cwd%` is substituted by the wrapper (`prelude/go/tools/go_wrapper.go:239-244`). So `runtime.Caller` and embedded file names are project-relative. The binary's content should not carry the checkout path, but this was not checked on a built binary.

**Hazards**

- **G1: absolute binary path in `command`.** It renders as `<checkout>/buck-out/.turnkey/…/__<name>__/<name>`.
  - Severity: **spurious miss (certain)**, on all 12 targets.
  - Fix kind: rule change. Either patch the prelude through `nix/patches/prelude/` so `go_test` sets both flags to `True` (the prelude's own FIXME), or have `tk` pass `--unstable-allow-all-tests-on-re`. Stripping the project-root prefix when computing the key is the fallback.
- **G2: `godeps_test` fixtures are not declared** (`src/go/pkg/godeps/rules.star:24-37`).
  - `findTestdataDir` (`src/go/pkg/godeps/integration_test.go:91-135`) tries three sources in turn, and all of them fail under buck2:
    - `<exeDir>/godeps_fixtures/godeps` (`:98-107`) is absent, because no resource is declared.
    - The cwd-relative candidates (`:111-124`) resolve inside buck-out, because of the `chdir` above.
    - `BUCK_PROJECT_ROOT` (`:127-132`) is never set, because it is not on the allowlist.
  - The test then calls `t.Skip` (`:134`), so `TestIntegration_{Simple,Medium,EdgeCases}` skip deterministically and the target reports a pass.
  - The fixtures exist as `//src/testdata:godeps_fixtures` (`src/testdata/rules.star:5-9`), and the lookup at `:104` was written for exactly that resource.
  - Severity: no wrong hit today, because the result does not depend on the fixtures. It becomes **wrong hit (latent)** if someone fixes the lookup without declaring the resource.
  - Fix kind: declare as a resource (`resources = ["//src/testdata:godeps_fixtures"]`) and turn the skip into a failure.
- **G3: `staleness_test` reads `$HOME`.** The chain is `DefaultCachePath` → `os.UserHomeDir()` (`src/go/pkg/staleness/cache.go:287-300`), asserted at `src/go/pkg/staleness/cache_test.go:293-305`. `HOME` is inherited (§3.3). Only "set and absolute" matters.
  - Severity: **wrong hit (negligible)**.
  - Fix kind: set `HOME` in the test with `t.Setenv`.
- **G4: `staleness_test` orders file mtimes with a 10 ms sleep.**
  - Where: `staleness_test.go:40, 68, 95, 128, 158, 166`, checked with a strict `After` (`staleness.go:96, 101`). The result depends on the timestamp resolution of the `$TMPDIR` filesystem.
  - Severity: a flake that a cache would freeze (low).
  - Fix kind: use `os.Chtimes`, as `src/go/pkg/rules` already does.
- **G5: `syncconfig_test` stats the host path `/nonexistent/.turnkey/sync.toml`** (`src/go/pkg/syncconfig/syncconfig_test.go:181`).
  - Severity: **wrong hit (negligible)**.
- **Clean.** No undeclared reads were found in `buckgen`, `cellfresh`, `extraction`, `goparse`, `localconfig`, `mapper`, `prefetchcache`, `rules` or `starlark`, which are in-memory or `t.TempDir()` only:
  - `cellfresh_test` replaces `KillCommand`, so `buck2 kill` never runs (`src/go/pkg/cellfresh/cellfresh_test.go:9-13`).
  - The `prefetch.go` network and `nix-prefetch-*` paths are not reached, because tests use `MockPrefetcher` and `prefetchcache.WithDir(t.TempDir())`.

### 4.2 `python_test` (4 targets)

**Declared inputs and `ExternalRunnerTestInfo`**

- **`command`** is `[<target>.pex]`. Its hidden inputs are the link tree and runtime artifacts.
- **`env`** is `ctx.attrs.env` plus `PYTHON_SOURCE_MAP = <dbg source db artifact>` (`prelude/python/compute_providers.bzl:35-59`). `resources` go into the link tree. No turnkey target sets `env`, `resources` or `run_env`.
- **Path flags:** both are `False` (`compute_providers.bzl:56-57`), for the reason in §3.2.
- **The runner is the prelude's unittest main, not pytest** (`prelude/python/python_test.bzl:39, 50`; `prelude/python/tools/__test_main__.py:35`). The `pytest` → `uv run pytest` shim (`nix/packages/pytest-uv-shim.nix:32`) is only placed on the dev-shell PATH (`nix/devenv/turnkey/buck2.nix:108-115`). So `pyproject.toml` pytest config, `conftest.py`, `.pytest_cache` and `.venv` are **not** read by buck2 tests.
- **The interpreter is PATH-resolved.** Turnkey's `system_python_toolchain` target sets no `interpreter` (`nix/buck2/mappings.nix:201-206`), so it defaults to the string `"python3"` (`prelude/toolchains/python.bzl:44-47, 112`) with `package_style = "inplace"` (`:101`). The `.pex` bootstrap is `#!/usr/bin/env python3` (`prelude/python/tools/make_py_package_inplace.py:171-176`). It re-execs `sys.executable` with empty interpreter flags (`prelude/python/tools/run_inplace.py.in:18, 143, 212`). It drops `''` from `sys.path` (`run_inplace.py.in:95-107`), so the checkout root is not importable.

**Hazards**

- **P1: interpreter from PATH, not in the key.**
  - A `python-toolchain` bump in `toolchain.toml` changes neither the toolchains-cell `BUCK` for Python nor any input digest.
  - The daemon keeps its PATH snapshot until restarted (§3.3). A daemon started outside `direnv` would use `/usr/bin/python3`.
  - Severity: **wrong hit (medium)**.
  - Fix kind: toolchain change. Set `interpreter` to a store path in `mappings.nix`, as the mdbook mapping already does for `python_path` (`nix/buck2/mappings.nix:416`).
- **P2: absolute paths in the key.** The `.pex` path in `command` and the `PYTHON_SOURCE_MAP` value in `env` both render as `<checkout>/buck-out/.turnkey/…`.
  - Severity: **spurious miss (certain)**.
  - Fix kind: same as G1.
- **P3: host and user site-packages are importable.**
  - Cause: `HOME` is inherited, and the interpreter runs without `-s`, `-E` or `-I` (`run_inplace.py.in:18, 143`).
  - Effect: `~/.local/lib/python3.x/site-packages`, `.pth` files and `sitecustomize` are on the path. An undeclared third-party import would resolve silently. `__test_main__.py` try-imports `coverage`.
  - Current state: none of the four targets imports anything beyond the stdlib and the declared `turnkey.*` modules.
  - Severity: **wrong hit (latent)**.
  - Fix kind: rule or toolchain change (`-s`/`-I`, or a pinned interpreter).
- **P4: link-tree symlinks into `pydeps//`.**
  - Link targets are `relpath(realpath(src), realpath(dest.parent))` (`make_py_package_modules.py:244-254`). For a `/nix/store` source, the `../` count depends on the depth of the checkout directory.
  - In-repo sources give checkout-independent relative links.
  - Current state: none of the four targets depends on `pydeps//`.
  - Severity: **spurious miss (latent)**.
  - Fix kind: rule change.
- **P5: runtime `.pyc` writes into the buck-out link tree.**
  - Cause: `compile` defaults to `False` (`prelude/python/python_test.bzl:62`), and bytecode writes are suppressed only if `PYTHONDONTWRITEBYTECODE` is set (`run_inplace.py.in:155-158`). That variable is not on the allowlist.
  - This matters only if a cache hashes the link tree from disk instead of using buck2's digests.
  - Severity: **spurious miss (low)**.
  - Fix kind: set `PYTHONDONTWRITEBYTECODE` in declared env, or key on buck2 digests only.
- **P6: two harmless host reads.**
  - `test_toml`: `find_workspace_root` walks every ancestor of the resolved `$TMPDIR` up to `/` and reads any `Cargo.toml` it finds (`src/python/cargo/turnkey/cargo/toml.py:27-35`, via `src/python/cargo/tests/test_toml.py:73-85`).
  - `test_features`: stats `/nonexistent/path.toml` (`src/python/cargo/tests/test_features.py:130-132`).
  - Severity: **wrong hit (negligible)**.

`//src/python/cfg:test` and `//src/examples/python-hello:python-hello-test` are pure: no filesystem, env, clock or subprocess use. Only P1–P3 and P5 apply to them.

### 4.3 `rust_test` (8 targets)

**Declared inputs and `ExternalRunnerTestInfo`**

- **`command`** is `cmd_args(final_output, hidden = runtime_files)` (`prelude/rust/rust_binary.bzl:228`). `resources`, and those of cxx deps, are gathered into a resources JSON plus hidden inputs (`rust_binary.bzl:129-133, 246-257`).
- **`env`** is `ctx.attrs.env | ctx.attrs.run_env`.
- **Path flags:** both are hard-coded `True` (`rust_binary.bzl:545-555`). No turnkey target sets `env`, `run_env` or `resources`, so the key is one project-relative path, an empty env and the input digests.
- **Sources compile from a symlink tree** (`__srcs`, `prelude/rust/sources.bzl:88-105`). So a relative `include_str!`/`include_bytes!` of an undeclared file fails the build rather than reading the checkout.
- **Compile paths are remapped.** `--remap-cwd-prefix=.` is on unless `nightly_features` is set (`prelude/rust/build.bzl:1464`), which keeps the checkout path out of the binary.
- **`rustc` comes from PATH** (`prelude/toolchains/rust.bzl:45`). That is a build-cache concern; the test key sees its effect through the binary digest.
- **`CARGO_MANIFEST_DIR` is never set by the prelude.** If a target put it in `env`, the compile-time value would be made absolute (checkout-specific) inside the binary. No turnkey target does this.
- **`libtest` variables are not inherited.** `RUST_TEST_THREADS` and similar do not reach tests (§3.3).

**Hazards**

- **R1: `composition-test` reads host `/tmp` metadata and entries** (`src/rust/composition/src/performance.rs:685-729`). The code is guarded by `exists()` or `if let Ok`, and asserts only `is_ok()`.
  - Severity: **wrong hit (negligible)**.
- **Other notes (no severity):**
  - `synthetic.rs:195-203` checks whether `/firefly` exists, but its assertion is a tautology.
  - Timing assertions in `recovery.rs` and `state.rs` are deterministic.
- **Vacuous passes (coverage notes, not caching hazards):**
  - `//src/cmd/check-rust-edition-rs:…-test` and `//src/cmd/check-source-coverage-rs:…-test` compile binaries whose only source is `main.rs`, with no `#[cfg(test)]` module. Their `current_dir` and `rules.star` reads run only when they are invoked as binaries.
  - In `//src/rust/composition:integration-tests`, `mod fuse_tests` is behind `#[cfg(feature = "fuse")]`, which the target does not enable (`src/rust/composition/tests/integration_tests.rs:262`).
- **`//e2e/fixtures/multi-language/rust_lib:greeting-test` is part of turnkey's own build.** The generated `.buckconfig` has no `[project] ignore` (`nix/devenv/turnkey/buck2.nix:341-368`), and its deps (`rustdeps//vendor/serde`, `serde_json`) exist in turnkey's rustdeps cell. The test itself is a pure serde round-trip.

### 4.4 `solidity_test` (2 targets)

**Declared inputs and `ExternalRunnerTestInfo`** (`nix/buck2/prelude-extensions/solidity/solidity_test.bzl`)

- **`command`** is the generated `run_tests.sh` artifact (`:42, 168-172`), followed by:
  - `toolchain.forge.args`: a `/nix/store/…-foundry-v1.7.1/bin/forge` string (`:176`; `nix/buck2/mappings.nix:238`);
  - `--test-srcs <srcs>` (`:179-181`);
  - `--dep-srcs`, which gets each `SolidityLibraryInfo` dep's *direct* `srcs` plus its compiled `output_dir`, or a filegroup's default outputs (`:21-36, 184-186`);
  - `--soldeps-cell .turnkey/soldeps`, the literal from `read_root_config("cells", "soldeps")` (`:188-192`).
- No `env`, no `resources`. The path flags are unset, so both default to `True` (`:197-204`).
- **What the script does:**
  - Makes `WORK_DIR=$(mktemp -d)` (`:79-80`).
  - Copies test and dep sources into `test/` and `src/`, and symlinks dep directories into `lib/` (`:112-126`).
  - Turns each line of `$SOLDEPS_CELL/remappings.txt` into an absolute `realpath` (`:128-143`).
  - Writes a minimal `foundry.toml` with only `src`, `test`, `libs` and `out` (`:154-161`).
  - Runs `cd "$WORK_DIR"; "$FORGE" test` (`:163-165`).
- **Consequence:** the repo's `foundry.toml`, the examples' `foundry.toml` files (which pin `solc_version = "0.8.33"`), and any `lib/`, `cache/` or `out/` in the checkout are **not** read by the tests. forge's `cache/` and `out/` are created fresh inside `WORK_DIR` on every run.

**Hazards**

- **S1: solc selection is outside the key and can use the network.**
  - The generated `foundry.toml` sets neither `solc` nor `offline`. The toolchain's `solc_path` (`nix/buck2/mappings.nix:237`) is never passed to forge.
  - foundry's defaults apply: `auto_detect_solc: true`, `offline: false` (`foundry@v1.7.1:crates/config/src/lib.rs:257-266, 2658-2659`). forge detects the pragma (`^0.8.20` in both tests) and picks a matching solc from svm's data dir, installing one from `binaries.soliditylang.org` if needed.
  - svm's data dir is `~/.svm` if that exists, otherwise `dirs::data_dir()/svm` (`svm-rs@v0.5.25:crates/svm-rs/src/paths.rs:40-67`). On this machine that is `~/Library/Application Support/svm`, holding 0.8.15, 0.8.16, 0.8.19, 0.8.25, 0.8.26 and 0.8.28–0.8.30.
  - The pinned forge binary embeds svm-rs, `.svm` and `binaries.soliditylang.org/bin/list.txt` (checked with `strings`).
  - The library targets, by contrast, compile with the Nix solc 0.8.34 (`nix/buck2/prelude-extensions/solidity/solidity_library.bzl:16-23, 143-146`).
  - The exact version-choice rule lives in foundry-compilers and was not traced here.
  - Severity: **wrong hit (medium)**. The compiler changes whenever the host's svm directory changes; also nondeterministic across machines.
  - Fix kind: rule change. Pass the toolchain solc to forge (for example `solc = "<store path>"` and `offline = true` in the generated config) so that it is in the command.
- **S2: `~/.foundry/foundry.toml` is merged if it exists.**
  - Source: `foundry@v1.7.1:crates/config/src/lib.rs:900-907, 2088-2096`. `HOME` is inherited. The file is absent on this machine.
  - `FOUNDRY_*` and `DAPP_*` env providers (`lib.rs:915-934`) are harmless under buck2, because those variables are not on the allowlist.
  - Severity: **wrong hit (latent)**.
  - Fix kind: rule change. Run forge with a throwaway `HOME`, or with a config that pins every setting that matters.
- **S3: the soldeps cell is read through a string, not an artifact.**
  - `remappings.txt` and every remapped file are read through the `.turnkey/soldeps` symlink (`:128-143`), and forge resolves `@openzeppelin/...` imports from there.
  - For `token_test`: `MyToken.sol` imports three OpenZeppelin files (`src/examples/solidity-hello-deps/src/MyToken.sol:4-6`). The test's own inputs include only `:token_lib`'s direct `srcs` and `artifacts` dir. `:token_lib` declares `soldeps//:openzeppelin_contracts` (`src/examples/solidity-hello-deps/rules.star:10-12`), but `solidity_test` does not propagate filegroup deps from `SolidityLibraryInfo`.
  - A soldeps change is caught only indirectly, through the recompiled `artifacts` dir, and only after the daemon restart covered in §3.5.
  - For `counter_test`, the cell is read but does not affect the result.
  - The literal relative string is what enters the key, so cross-checkout reuse is unaffected.
  - Severity: **wrong hit (low)**.
  - Fix kind: rule change. Propagate transitive dep sources as inputs, or pass the cell's remappings and filegroups as artifacts instead of a path string.
- **S4: fuzzing is unseeded.** Both targets set `fuzz_runs = 256` and contain `testFuzz_*` functions (`src/examples/solidity-hello/test/Counter.t.sol:44`, `src/examples/solidity-hello-deps/test/MyToken.t.sol:60`).
  - The rule never emits `--fuzz-seed` (`solidity_test.bzl:51-70`), and the generated config sets no `[fuzz] seed`. With no seed, forge builds a "stochastic fuzzer" through `TestRunner::new` (`foundry@v1.7.1:crates/forge/src/runner.rs:1192-1210`; `crates/config/src/fuzz.rs` `seed: Option<U256>`, default `None`).
  - A cached pass freezes one random sample.
  - Severity: **wrong hit (medium)**.
  - Fix kind: rule change (a seed derived from the key), or an opt-out label for fuzz and invariant tests.
- **S5: host tools come from PATH.** The script uses `#!/usr/bin/env bash`, `mktemp`, `cp`, `ln`, `realpath` and `basename` (`:72-152`).
  - Severity: **wrong hit (low)**.
  - Fix kind: rule change (a store-path interpreter and tools from the toolchain).
- **Build side, noted but out of scope.** `solidity_library` reads the same `.turnkey/soldeps` string (`solidity_library.bzl:152-156`). It declares the soldeps filegroups as hidden inputs (`:166-170`), so its own build key is mostly covered.

### 4.5 `jsonnet_test` (2 targets)

**Declared inputs and `ExternalRunnerTestInfo`** (`nix/buck2/prelude-extensions/jsonnet/jsonnet_test.bzl`)

- **Hidden inputs** are `src`, the `JsonnetLibraryInfo.sources` of every dep, and `golden` when it is set (`:124-130`).
- **`command`** is the `run_test.sh` artifact, then the `jrsonnet` store path (`nix/buck2/mappings.nix:459`), then `src` and `golden` (`:130-135`).
- **Script content:** `-J`, `--ext-str` and `--ext-code` values are baked into the script, which is itself an artifact (`:40-59, 64-116`).
- No `env`. The path flags are unset, so both default to `True` (`:140-147`). The key holds no checkout-specific value.
- **Where the import paths come from.**
  - Import paths come from `short_path` (`:31-35`).
  - For source artifacts, `short_path` is the *package-relative* path (`buck2:app/buck2_execute/src/path/artifact_path.rs:60-77`, compared with `with_full_path` at `:79-96`, which prepends the package).
  - So `config.jsonnet` gives `src_dir = "."`, and both tests get `-J .`. With cwd at the project root, that directory is the repo root, not the package directory.
  - The dep `:common` contributes `"."` as well (`jsonnet_library.bzl:26-29`).
- **Imports in the current tests are declared.** `common.libsonnet` is imported by both tests (`src/examples/jsonnet-config/common_test.jsonnet:4`, `config.jsonnet:5`) and comes in as a hidden input through `deps = [":common"]` (`src/examples/jsonnet-config/rules.star:35-48`). The golden file is declared.

**Hazards**

- **J1: undeclared imports resolve silently.** Jsonnet resolves an import relative to the importing file first, then against the `-J` directories. Sources are not materialized (§3.4), so an import of a file missing from `src` or `deps` reads the checkout in place. `-J .` also exposes every file at the repo root by bare name.
  - Severity: **wrong hit (latent)**.
  - Fix kind: rule change. Run jrsonnet against a `symlinked_dir` of the declared sources and derive `-J` from the full path.
- **J2: host tools come from PATH.** `#!/usr/bin/env bash` in both modes; `mktemp` and `diff` in golden mode (`:64-89`).
  - Severity: **wrong hit (low)**.
  - Fix kind: rule change.
- **Not a hazard: `JSONNET_PATH`.** The jrsonnet binary reads `JSONNET_PATH` (the string appears in `/nix/store/…-jrsonnet-0.5.0-pre96-test/bin/jrsonnet`), but that variable is not on the test allowlist.

## 5. Key hygiene outside the rules

- **Isolation dir.** Project-relative artifact paths contain the isolation dir: `buck-out/.turnkey/…`. `tk --isolation-dir=foo` rewrites it to `.turnkey-foo` (`src/cmd/tk/main.go:506-507, 629-670`). buck2 notes that a non-default isolation dir changes output paths.
  - Severity: **spurious miss (low)** between checkouts that use different isolation dirs.
- **`.turnkey/local.toml` overrides.** This file is per workspace and git-ignored (`src/go/pkg/localconfig/localconfig.go:30`; `.gitignore` `.turnkey/*`). Its overrides are injected after `--` for `tk test //target` (`src/cmd/tk/main.go:509-512, 553-593`), then reach the runner and the request (`buck2:app/buck2_test_runner/src/runner.rs:135-183`).
  - Keyed on the request digest, the result is correct: two workspaces with different overrides should not share results.
  - It becomes a **wrong hit** only if a key is built from `ExternalRunnerTestInfo` alone, before the runner args are merged.
- **Runtime-only values.** `PWD`, the absolute `argv[0]`, `BUCK_BUILD_ID` and `BUCK2_DAEMON_UUID` (§3.1) must stay out of any key. Including them would turn every lookup into a miss.

## 6. Fix kinds by hazard (not designs)

| Hazard | Kind of fix |
|---|---|
| G1, P2 (absolute paths in the key) | Rule change: prelude patch setting both flags to `True`, or `tk` forcing them through `--unstable-allow-all-tests-on-re`. Fallback: normalize the project-root prefix out of the key |
| P1, S5, J2 (tools from PATH) | Toolchain or rule change: store-path interpreter and tools, so the identity enters the command. Alternatively, add the resolved identities of PATH tools to the key |
| X1 (inherited env) | Key policy: record which of the six allowlisted variables each rule depends on, and key on their *resolved* meaning, not raw strings |
| X3 (stale cells) | Only cache under `tk`, after `cellfresh`; or put the `.turnkey/*` symlink targets (store paths, checkout-independent) in the key |
| G2 | Declare as a resource, and fail instead of skipping |
| S1, S2 | Rule change: pin solc from the toolchain, go offline, isolate `HOME` |
| S3, J1 | Rule change: declare transitive sources as artifacts and stage them, so undeclared reads fail |
| S4 | Rule change (seed derived from the key) or an opt-out label for fuzz tests |
| P3 | Rule or toolchain change (`-s`/`-I`) |
| P4, P5 | Rule change; or key only on buck2 digests, never on on-disk trees |
| G3, G4, G5, P6, R1 | Test-code fixes, or accept (negligible) |

## 7. Not covered or not verified

- **Build actions.** Their own undeclared inputs are out of scope: `go`, `rustc` and `python3` resolved from PATH at build time, and `GOCACHE` and `TMPDIR` in the Go wrapper. They affect test keys only through output digests.
- **forge's exact solc choice.** Installed versus remote, for a `^0.8.20` pragma. This lives in foundry-compilers and was not traced.
- **The devenv shell's actual `PATH`.** It was not inspected at runtime, so it is not confirmed that it holds no checkout-specific entries.
- **Go binary content.** That the Go test binaries carry no checkout path was inferred from `-trimpath %cwd%`, not checked on a built artifact.

## Sources

- **Turnkey:** this workspace at `main` (`50f53acc`). Paths are relative to the repo root.
- **Prelude:** the pinned prelude is the main checkout's `.turnkey/prelude` → `/nix/store/99d304la1zz7brjz73wl09xd3gvwvdaf-turnkey-prelude`. It is built by `nix/buck2/prelude.nix:40-54` from [facebook/buck2-prelude@27c8628d](https://github.com/facebook/buck2-prelude/tree/27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795) (the `upstreamPrelude` of `/nix/store/p9b6wjk0bj71dr0sry6dhqrrhmhrh7h4-turnkey-prelude.drv`), plus `nix/buck2/prelude-extensions/`. No patches: `nix/patches/prelude/` is empty.
- **buck2:** `2026-04-15`, the version in `buck2-toolchain` v3 from `toolchain.toml:6`. Citations `buck2:<path>:<line>` refer to [facebook/buck2@2026-04-15](https://github.com/facebook/buck2/tree/2026-04-15).
- **Foundry:** v1.7.1, the forge in the generated toolchains cell. Note that the comment at `toolchain.toml:14` says v1.5.1. Citations `foundry@v1.7.1:<path>:<line>` refer to [foundry-rs/foundry@v1.7.1](https://github.com/foundry-rs/foundry/tree/v1.7.1).
- **svm-rs:** v0.5.25, from foundry v1.7.1's `Cargo.lock`. [alloy-rs/svm-rs@v0.5.25](https://github.com/alloy-rs/svm-rs/tree/v0.5.25).
- **Generated files:** the toolchains cell `BUCK` and the buckconfig were read from the main checkout's `.turnkey/toolchains` and `.buckconfig` (Nix store). They are cited through their generators, `nix/buck2/mappings.nix` and `nix/devenv/turnkey/buck2.nix`.
