# Turnkey

Turnkey is a toolchain-as-code framework for Nix flakes. It turns declarative toolchain and dependency declarations into development shells and Buck2 cells for consuming repositories.

## Language

### Buck2

**Pinned buck2 release**:
The one buck2 release a turnkey revision ships: the binary, the prelude built with it, and the buck2 source revision they come from, always moved together. Consumers get it by choosing a turnkey revision, never by declaring buck2 themselves.
_Avoid_: buck2 version (as a consumer setting), declared buck2, supported buck2 versions

### Testing

**Test result caching**:
Not re-running a test whose inputs are identical to those of a recorded run, and reporting the recorded result instead, whether the test last ran locally or remotely. Buck2 reuses build action results in both cases but test results only when a remote worker ran the test; closing that gap is the point.
_Avoid_: test caching, incremental testing, build caching (which is the existing, separate behaviour for build actions)

**Target determination**:
Choosing, from a change, which tests *could* be affected before anything is built. Distinct from test result caching, which decides after the build whether a test's inputs actually changed.
_Avoid_: test selection, affected tests

**Recorded result**:
A test's pass, stored under its result key so that a later run can report it instead of running the test. Only passes are ever recorded results; a failure is always run again.
_Avoid_: cached failure, test cache entry

**Result key**:
Everything the test process can see (its inputs, argv, declared environment, timeout, working directory and platform), plus the versions of buck2 and of the caching tool. Two runs share a recorded result only if their result keys are equal.
_Avoid_: cache key, test hash

**Reuse policy**:
The rules, kept outside the result key, for when a recorded result may be read or written: only passes, only under `tk`, not for targets labelled `no-test-cache`, not read when a re-run is forced, and written only into the local cache. `tk` applies it for each run; the test runner only obeys the mode it is given. Changing the policy never splits the recorded results.

**Forced re-run**:
A `tk --rerun test` run: every test runs instead of reusing a recorded result, and fresh passes are still recorded into the local cache. Distinct from the `no-test-cache` label, which keeps a target out of caching altogether.
_Avoid_: `--no-test-cache` (the label's name), cache bypass

**Hit**:
A test run answered by a recorded result instead of running the test. Every hit is visible as such to the person running the tests. Its opposite is simply that the test ran.
_Avoid_: cached pass, cache hit (buck2 uses that for build actions too)

**Cache-safe rule**:
A test rule none of whose own behaviour lets a test read something outside its result key. Only a cache-safe rule has its targets' results recorded. Hazards that belong to one target, not to the rule, are fixed in that target or opt it out, and never make the rule unsafe.
