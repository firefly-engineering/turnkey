# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Opting a test rule into turnkey's test result caching.

A test rule passes the keyword arguments it would give ExternalRunnerTestInfo,
exactly as it builds them without caching, and, if it has them, the
RemoteTestExecutorConfig it got from `get_re_executors_from_props`, through
`test_caching_kwargs`:

    ExternalRunnerTestInfo(**test_caching_kwargs({
        "type": "rust",
        "command": [args],
        "env": env,
        "default_executor": re_executors.default_executor,
        ...
    }, re_executors))

Calling it is what makes a rule cache-safe (see CONTEXT.md); every decision
about what caching changes, and for which targets, is made here, so a
prelude patch only wraps upstream's arguments.

A target labelled `no-test-cache` is kept out of caching altogether: its
arguments are returned unchanged, so it runs on upstream's executor, which
never reads a recorded result, and turnkey's test runner never records it.

When the repo enables test result caching (the generated .buckconfig sets
`turnkey.test_cache = true`), every other target's arguments:

- declare the test cacheable (`supports_test_execution_caching`), and label
  it `turnkey-cacheable`, the only way turnkey's test runner can tell it
  may record the test's passes: buck2 reports an action digest for every
  test;
- run it on an executor that reads recorded results from the local test
  result cache, unless `re_executors` says upstream built a remote executor
  (from a `remote_execution` profile or the toolchain's default profile),
  which is kept. Upstream gives every other test an explicit local executor
  that never reads a cache, so that one is replaced. Build actions are
  unaffected: this executor only runs the test itself. It doesn't carry the
  target's `network_access` policy, so a test that needs the network
  shouldn't be cached (label it `no-test-cache`);
- render its command with project-relative paths from the project root, so
  other checkouts of the same revision share results;
- pin PATH to Nix store paths (`turnkey.test_path`) and HOME to a path that
  doesn't exist, so the test can't read tools or configuration that aren't in
  its result key. A target's own env still takes precedence.

Otherwise the arguments are returned unchanged. See
docs/specs/test-result-caching.md.
"""

load("@prelude//tests:re_utils.bzl", "RemoteTestExecutorConfig")

# Marks a test whose passes turnkey's test runner may record
# (src/go/pkg/testcache/testdata/runner-contract.json).
_CACHEABLE_LABEL = "turnkey-cacheable"

# Keeps a target out of test result caching (docs/specs/test-result-caching.md).
_NO_TEST_CACHE_LABEL = "no-test-cache"

# Nix's convention for "no home directory".
_HOMELESS = "/homeless-shelter"

def test_caching_enabled() -> bool:
    """Whether this repo has test result caching turned on."""
    return read_root_config("turnkey", "test_cache", "false") == "true"

def test_caching_opted_out(labels: list[str] | None) -> bool:
    """Whether a target with these labels is kept out of test result caching."""
    return _NO_TEST_CACHE_LABEL in (labels or [])

def test_caching_kwargs(
        kwargs: dict[str, typing.Any],
        re_executors: RemoteTestExecutorConfig | None = None,
        extra_path: list[str] = [],
        extra_env: dict[str, str] = {}) -> dict[str, typing.Any]:
    """Return ExternalRunnerTestInfo keyword arguments with caching enabled.

    `re_executors` is the rule's RemoteTestExecutorConfig; a rule without
    remote execution leaves it out. `extra_path` lists additional Nix store `bin` directories the rule's test
    needs on PATH, beyond the shared base (bash, coreutils, diffutils).
    `extra_env` adds variables the rule's tests need when cached, such as
    ones that stop them writing into their inputs. Like the pinned PATH and
    HOME, they apply only when caching is on, and a target's own env wins.
    """
    if not test_caching_enabled() or test_caching_opted_out(kwargs.get("labels")):
        return kwargs

    path = read_root_config("turnkey", "test_path", "")
    if not path:
        fail("turnkey.test_cache is enabled but turnkey.test_path is not set")
    pinned_env = extra_env | {
        "HOME": _HOMELESS,
        "PATH": ":".join(extra_path + [path]),
    }

    result = dict(kwargs)
    result["env"] = pinned_env | (kwargs.get("env") or {})
    result["supports_test_execution_caching"] = True
    result["labels"] = (kwargs.get("labels") or []) + [_CACHEABLE_LABEL]
    result["run_from_project_root"] = True
    result["use_project_relative_paths"] = True
    if re_executors == None or not re_executors.remote:
        result["default_executor"] = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = False,
            remote_cache_enabled = True,
        )
    return result
