# Turnkey

Turnkey is a toolchain-as-code framework for Nix flakes. It turns declarative toolchain and dependency declarations into development shells and Buck2 cells for consuming repositories.

## Language

### Testing

**Test result caching**:
Not re-running a test whose inputs are identical to those of a recorded run, and reporting the recorded result instead, whether the test last ran locally or remotely. Buck2 reuses build action results in both cases but test results only when a remote worker ran the test; closing that gap is the point.
_Avoid_: test caching, incremental testing, build caching (which is the existing, separate behaviour for build actions)

**Target determination**:
Choosing, from a change, which tests *could* be affected before anything is built. Distinct from test result caching, which decides after the build whether a test's inputs actually changed.
_Avoid_: test selection, affected tests
