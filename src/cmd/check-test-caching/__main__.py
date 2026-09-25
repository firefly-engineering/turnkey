#!/usr/bin/env python3
"""Check what the test-caching helper gives each kind of test target.

Analyses sample test targets of every cache-safe rule with test result
caching on and off, through check.bxl, and compares what buck2 sees on each
target's ExternalRunnerTestInfo with the expected table:

- caching on: the test is declared cacheable, runs from the project root
  with project-relative paths, and gets the cache-reading executor, unless
  it has a remote_execution profile, whose executor it keeps;
- caching off: the executor is the rule's own, as upstream builds it.

re-profile-test's profile is set on the target. A profile from the remote
test execution toolchain's default takes the same path through upstream's
get_re_executors_from_props, which is where the helper learns an executor
is remote, so it isn't a separate fixture.

This tests nix/buck2/prelude-extensions/test_caching/test_caching.bzl
through the only interface that matters, the provider buck2 reads; the
prelude patches that call it are only wrapping.

Usage (from the repo root, in the dev shell):
    python3 src/cmd/check-test-caching/__main__.py

Exits 0 when every target matches, 1 otherwise.
"""

import json
import subprocess
import sys

BXL = "//src/cmd/check-test-caching/check.bxl:main"

# Gives re-profile-test a remote_execution profile (see rules.star).
RE_PROFILE = ["-c", "turnkey.check_test_caching_re_profile=true"]

# target -> (executor with caching on, executor with caching off)
EXPECTED_EXECUTORS: dict[str, tuple[str, str]] = {
    "root//src/examples/rust-hello:rust-hello-test": ("cache", "local"),
    "root//src/go/pkg/rules:rules_test": ("cache", "local"),
    "root//src/python/cfg:test": ("cache", "local"),
    "root//src/examples/jsonnet-config:common-test": ("cache", "none"),
    "root//src/examples/solidity-hello:counter_test": ("cache", "none"),
    "root//src/cmd/check-test-caching:re-disabled-test": ("cache", "local"),
    # A remote_execution profile's executor is kept either way.
    "root//src/cmd/check-test-caching:re-profile-test": ("other", "other"),
}

CACHING_FIELDS = (
    "supports_test_execution_caching",
    "run_from_project_root",
    "use_project_relative_paths",
)


def report(caching: bool) -> dict[str, dict]:
    """What check.bxl reports for every target, with caching on or off."""
    result = subprocess.run(
        [
            "buck2",
            "bxl",
            "-c",
            f"turnkey.test_cache={'true' if caching else 'false'}",
            *RE_PROFILE,
            BXL,
            "--",
            "--targets",
            *EXPECTED_EXECUTORS,
        ],
        stdout=subprocess.PIPE,
        check=True,
        text=True,
    )
    return json.loads(result.stdout)


def mismatches(caching: bool, actual: dict[str, dict]) -> list[str]:
    found = []
    for target, (on, off) in EXPECTED_EXECUTORS.items():
        info = actual.get(target)
        if info is None:
            found.append(f"{target}: not reported")
            continue
        expected = on if caching else off
        if info["executor"] != expected:
            found.append(f"{target}: executor {info['executor']}, expected {expected}")
        for name in CACHING_FIELDS:
            if caching and info[name] is not True:
                found.append(f"{target}: {name} is {info[name]}, expected True")
        if not caching and info["supports_test_execution_caching"] is True:
            found.append(f"{target}: declared cacheable with caching off")
    return found


def main() -> int:
    failures = 0
    for caching in (True, False):
        mode = "on" if caching else "off"
        found = mismatches(caching, report(caching))
        for line in found:
            print(f"caching {mode}: {line}")
        failures += len(found)
    targets = len(EXPECTED_EXECUTORS)
    if failures:
        print(f"check-test-caching: {failures} mismatch(es) across {targets} targets")
        return 1
    print(f"check-test-caching: {targets} targets match with caching on and off")
    return 0


if __name__ == "__main__":
    sys.exit(main())
