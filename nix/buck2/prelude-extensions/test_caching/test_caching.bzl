# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Opting a test rule into turnkey's test result caching.

A test rule passes the keyword arguments it would give ExternalRunnerTestInfo
through `test_caching_kwargs`:

    ExternalRunnerTestInfo(**test_caching_kwargs({
        "type": "rust",
        "command": [args],
        "env": env,
    }))

When the repo enables test result caching (the generated .buckconfig sets
`turnkey.test_cache = true`), the returned arguments:

- declare the test cacheable (`supports_test_execution_caching`);
- run it on an executor that reads recorded results from the local test
  result cache, unless the rule already chose an executor. Build actions are
  unaffected: this executor only runs the test itself;
- render its command with project-relative paths from the project root, so
  other checkouts of the same revision share results;
- pin PATH to Nix store paths (`turnkey.test_path`) and HOME to a path that
  doesn't exist, so the test can't read tools or configuration that aren't in
  its result key. A target's own env still takes precedence.

Otherwise the arguments are returned unchanged. See
docs/specs/test-result-caching.md.
"""

# Nix's convention for "no home directory".
_HOMELESS = "/homeless-shelter"

def test_caching_enabled() -> bool:
    """Whether this repo has test result caching turned on."""
    return read_root_config("turnkey", "test_cache", "false") == "true"

def test_caching_kwargs(
        kwargs: dict[str, typing.Any],
        extra_path: list[str] = [],
        extra_env: dict[str, str] = {}) -> dict[str, typing.Any]:
    """Return ExternalRunnerTestInfo keyword arguments with caching enabled.

    `extra_path` lists additional Nix store `bin` directories the rule's test
    needs on PATH, beyond the shared base (bash, coreutils, diffutils).
    `extra_env` adds variables the rule's tests need when cached, such as
    ones that stop them writing into their inputs. Like the pinned PATH and
    HOME, they apply only when caching is on, and a target's own env wins.
    """
    if not test_caching_enabled():
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
    result["run_from_project_root"] = True
    result["use_project_relative_paths"] = True
    if result.get("default_executor") == None:
        result["default_executor"] = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = False,
            remote_cache_enabled = True,
        )
    return result
