# Test Result Caching

`tk test` reuses a test's recorded result when nothing it depends on has
changed, instead of running it again. buck2 already caches build actions;
test result caching extends that to test runs. It is on by default.

```console
$ tk test //src/...
Tests finished: Pass 42. Fail 0. Timeout 0. Fatal 0. Skip 0. Omit 0. Infra Failure 0. Build failure 0
38 recorded (reused without running)
```

A reused result is a **hit**. The last line counts the hits. Exit codes are
the same as when every test runs: `0` if all tests pass, `32` if any fails.

Passing tests, hits included, aren't listed: only tests that didn't pass are,
with their output. To list every test with its output, pass
`--print-passing-details` to the test runner:

```console
$ tk test //src/... -- --print-passing-details
✓ Pass: root//src/rust/starlark-parse:starlark-parse-test
recorded: reused the result of an earlier run with the same inputs
---- STDOUT ----
...
```

A hit is then marked `recorded` under the test's line, shows no duration (the
test didn't run), and prints the output of the run that recorded it.

## When a result is reused

A result is reused only when its **result key** is unchanged. The key covers
everything the test can see:

- its inputs (sources, dependencies, data files, the toolchain);
- its command line, including arguments after `--` such as `--test-arg`
  (so a filtered run has its own key);
- its declared environment, including `--env`;
- its timeout, working directory and platform;
- the buck2 release and the caching tool's version.

It works across buck2 daemon restarts, `tk clean`, and other checkouts of the
same revision on the same machine (jj workspaces, git worktrees).

Only **passes** are recorded. A test that failed, timed out or crashed always
runs again.

Caching applies only under `tk test`. A plain `buck2 test` neither reuses nor
records results.

### Forcing a re-run

```bash
tk --rerun test //src/...
```

runs every matched test, and records the fresh passes, which replace the old
ones. Like `--no-sync`, the flag goes before the subcommand.

### Which targets are cached

Every target of a cache-safe rule is: turnkey's rules and the rust, go and
python test rules. With caching on, they carry the label `turnkey-cacheable`,
which `buck2 uquery` shows. It is added by the rules, never by hand.

### Opting a target out

Label a target `no-test-cache` to always run it and never record it, for
example a test that talks to the network or depends on the time:

```python
go_test(
    name = "integration_test",
    srcs = ["integration_test.go"],
    # Talks to a staging server.
    labels = ["no-test-cache"],
)
```

For `solidity_test`, the label also switches fuzzing back to a random seed;
cached Solidity tests fuzz with a seed derived from the target's label. A
`solidity_test` with `fork_url` is never cached, because it reads chain state
over the network.

## What each language does

A test is only cached when its rule can't read anything outside its result
key. For cached tests, turnkey:

- sets `PATH` to Nix store paths only (bash, coreutils, diffutils), and
  `HOME` to `/homeless-shelter`, a directory that doesn't exist. A test that
  needs a writable directory should use `$TMPDIR`;
- runs the command with project-relative paths, from the project root.

| Language | Notes |
|---|---|
| Rust | No other changes. |
| Go | Tests that read fixtures must declare them as `resources`. |
| Python | Runs the toolchain's interpreter by store path, with `PYTHONDONTWRITEBYTECODE=1`. |
| Jsonnet | `import` only resolves from the test's declared sources and dependencies. |
| Solidity | forge uses the toolchain's `solc`, offline, with dependencies from the soldeps cell. |

## Configuration

Test result caching is configured in your flake, under the Buck2 integration:

```nix
turnkey.toolchains.buck2.testCache = {
  enable = true;       # default; false runs tests under buck2's bundled runner
  endpoint = null;     # default: a cache on this machine, managed by tk
  tls = true;          # only used with a remote endpoint
};
```

## The local cache

Recorded results live in one store per user per machine, shared by every
checkout and every turnkey repo:

| Platform | Location |
|---|---|
| macOS | `~/Library/Caches/turnkey/test-results/` |
| Linux | `~/.cache/turnkey/test-results/` |

`tk test` starts the cache server (bazel-remote) on demand, in the
background. It stops by itself after 24 hours without use, and the next
`tk test` starts it again; recorded results stay in the store. Its log is
`server.log` in the store. The store is limited to 5 GiB; the least recently
used results are dropped first.

If the cache can't be reached or started, `tk test` runs the tests uncached
and prints one line:

```text
tk: running tests without the test result cache: <reason>
```

### Per-user settings

These are set in your environment, not in the repo:

| Variable | Effect |
|---|---|
| `TURNKEY_CACHE_DIR` | Keep turnkey's caches here instead of the platform cache directory. |
| `TURNKEY_TEST_CACHE_SIZE_GIB` | Store size limit in GiB (default 5). |
| `TURNKEY_TEST_CACHE_PORT` | Port of the local cache server (default 47301). Read when the dev shell is evaluated, so reload the shell after changing it. |

## A remote cache

Set `endpoint` to use a shared Remote Execution API cache instead of the
local one:

```nix
turnkey.toolchains.buck2.testCache.endpoint = "grpc://cache.example.com:443";
```

`tk test` then starts no local cache, and results found there are marked
`recorded, remote`. Nothing is recorded to a remote cache yet: who may write
to a shared cache hasn't been decided. `tk` can't check a remote cache
before running, so if it is unreachable, buck2 retries for about 45 seconds
and then runs the tests locally, uncached.

## In the event log

For every hit, the test result in buck2's event log (`buck2 log show`)
carries a JSON record in its `msg` field:

```json
{"turnkey_test_cache": {"hit": true, "origin": "local", "original_duration_us": 475340}}
```

`origin` is `local` or `remote`, and `original_duration_us` is how long the
recorded run took. The test's `TestEnd` event reports the execution as a
`RemoteCommand` with `cache_hit: true`.

## Caching your own test rules

A custom test rule opts in by passing the arguments it gives
`ExternalRunnerTestInfo` through `test_caching_kwargs`:

```python
load("@prelude//test_caching:test_caching.bzl", "test_caching_kwargs")

def _my_test_impl(ctx):
    command = cmd_args(ctx.attrs.runner[RunInfo], ctx.attrs.src)
    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(**test_caching_kwargs(
            {
                "type": "my_language",
                "command": [command],
                "env": ctx.attrs.env,
                "labels": ctx.attrs.labels,
            },
            # Store-path bin directories the test needs beyond bash,
            # coreutils and diffutils.
            extra_path = [],
            # Variables only cached runs need.
            extra_env = {},
        )),
    ]
```

When caching is off, the arguments come back unchanged. When it is on, the
helper also gives the test an executor that reads recorded results. A rule
that supports remote execution passes its `re_executors` as well, and a test
that upstream runs remotely keeps its executor.
Only opt a rule in
when its tests can't read anything that isn't in their result key: every
file they read must be a declared input, and every tool must come from a
store path.
