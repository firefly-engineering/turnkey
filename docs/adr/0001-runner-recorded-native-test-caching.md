---
status: accepted
---

# Test result caching: buck2's native lookup, results recorded by our own test runner

buck2 can serve a test's result from a remote-execution action cache, but it never uploads a test that ran locally, so on its own it gives local hits only for tests a remote worker ran ([research](../research/buck2-pinned-test-caching.md)). We keep buck2's native lookup and fill the gap from outside buck2. Cache-enabled tests use an executor that reads from a local cache-only REAPI server (bazel-remote). A test runner we own, which replaces buck2's bundled OSS runner, writes an `ActionResult` for each **passing** local run under the action digest buck2 itself computed and reported to it. Hits come back as native cache hits, which the runner reports as `recorded`. The result key is therefore always buck2's own digest, never a second one we compute. Sharing results across machines later only means pointing the same executor at a remote endpoint. Decided in `turnkey-w55.7`. The facts behind it are in [test-cache-mechanism-feasibility.md](../research/test-cache-mechanism-feasibility.md).

## Considered options

- **Local remote-execution workers.** Rejected. On darwin the only candidates are NativeLink (FSL licence, not in nixpkgs) and Buildbarn (a platform-property mismatch with buck2). Tests would also run in a worker's directory, with no sandboxing on darwin.
- **Tests as build actions** (the test writes its outcome to a file, and a trivial test replays it). Rejected on two counts:
  - A `run` action is only uploaded when it succeeds, so a failing test either becomes a build failure or gets recorded as a failure.
  - No supported flag skips reads but still uploads, so a forced re-run can't re-record.

  Build actions also inherit the daemon's whole environment, which gives more hidden inputs than a test's allowlist.
- **A cache owned entirely by the runner**, with its own key and store. Rejected: it creates a second source of truth for the key, and it doesn't speak a standard REAPI endpoint.
- **Patching buck2** to upload passing local test runs: an estimated 30–40 lines, see [facebook/buck2#183](https://github.com/facebook/buck2/issues/183). Kept in reserve. It means building buck2 from source, where turnkey ships the prebuilt release today, and rebasing the patch on every bump. If upstream ever uploads local test runs itself, the runner's write step goes away and nothing else changes.

## Consequences

- **The runner speaks buck2's test-runner protocol,** so it is pinned to the exact buck2 version turnkey ships. It must match everything the OSS runner does (`--env`, `--timeout`, `--test-arg`, stdout and stderr in the result details, exit code 32 on failure). The timeout it sends is part of the digest, so it has to be derived deterministically.
- **Recorded entries must be well formed.** They are written only for passes, always carry `execution_metadata` (buck2 takes a hit's duration from it), use buck2's `instance_name`, and name outputs by project-relative path.
- **The cache-reading executor goes on the test provider's `default_executor`, not the execution platform.** Build actions therefore never touch the cache.
- **The runner is off unless `tk test` turns it on.** A plain `buck2 test` neither reads nor records. An unreachable cache server makes buck2 retry for about 45 s and then report every opted-in test as failed, without running it (verified in `turnkey-w55.10`). So the cache's reachability is checked before a run, and when it's down the tests run uncached rather than fail.
- **Cross-checkout hits need project-relative test commands.** The executor does not change how buck2 renders paths.
