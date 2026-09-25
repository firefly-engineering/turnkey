#!/usr/bin/env python3
"""Check that turnkey-test-runner behaves exactly like buck2's bundled runner.

Runs `buck2 test` on the same targets twice per scenario, once with buck2's
bundled runner (`-c test.v2_test_executor=`) and once with turnkey's, and
compares, from buck2's event log:

- the exit code;
- every target's test status;
- every target's action digest. buck2 computes it from the request the
  runner sends (command, env, timeout, ...), so equal digests mean the
  runners asked for the same execution.

This is the gate for supporting a buck2 release: add a release to
nix/buck2/buck2-source.nix only once this passes against it.

Usage (from the repo root, in the dev shell):
    python3 src/cmd/check-test-runner-parity/__main__.py [TARGET_PATTERN ...]

Exits 0 when every scenario matches, 1 otherwise.
"""

import json
import subprocess
import sys
from dataclasses import dataclass, field

# Runner arguments passed after `--`, exercising each option the runner
# must handle like buck2's (`--test-arg` last: it consumes what follows).
SCENARIOS: dict[str, list[str]] = {
    "defaults": [],
    "env-and-timeout": ["--env", "TURNKEY_PARITY_PROBE=1", "--timeout", "300"],
    "test-arg": ["--test-arg", "--turnkey-parity-probe"],
}

# Both runs turn test result caching off in the rules, so they build the
# same test commands: cache-enabled rules would make buck2 look results up
# for the bundled runner, which never disables caching.
RULES_WITHOUT_CACHING = ["-c", "turnkey.test_cache=false"]
BUNDLED_RUNNER = ["-c", "test.v2_test_executor=", *RULES_WITHOUT_CACHING]
TURNKEY_RUNNER = RULES_WITHOUT_CACHING


@dataclass
class Run:
    exit_code: int
    statuses: dict[str, int] = field(default_factory=dict)
    digests: dict[str, str] = field(default_factory=dict)


def target_name(label: dict) -> str:
    return f"{label['package']}:{label['name']}"


def run(patterns: list[str], config: list[str], runner_args: list[str]) -> Run:
    """Run `buck2 test`, then read its results from buck2's event log.

    buck2 directly, not `tk test`: tk adds turnkey-test-runner's caching
    flags after `--`, which buck2's bundled runner rejects. Without them
    turnkey's runner neither reads nor records, like the bundled one.
    """
    test = subprocess.run(
        ["buck2", "test", *config, *patterns, "--", *runner_args],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    log = subprocess.run(
        ["buck2", "log", "show"], capture_output=True, text=True, check=True
    )
    result = Run(exit_code=test.returncode)
    for line in log.stdout.splitlines():
        try:
            event = json.loads(line)["Event"]["data"]
        except (json.JSONDecodeError, KeyError, TypeError):
            continue
        instant = event.get("Instant", {}).get("data", {})
        if "TestResult" in instant:
            test_result = instant["TestResult"]
            result.statuses[test_result["name"]] = test_result["status"]
        test_end = event.get("SpanEnd", {}).get("data", {}).get("TestEnd")
        if test_end:
            name = target_name(test_end["suite"]["target_label"]["label"])
            command = test_end["command_report"]["details"]["command_kind"]["command"]
            for kind in command.values():
                if "action_digest" in kind:
                    result.digests[name] = kind["action_digest"]
    return result


def compare(bundled: Run, turnkey: Run) -> list[str]:
    problems = []
    if bundled.exit_code != turnkey.exit_code:
        problems.append(
            f"exit code: bundled {bundled.exit_code}, turnkey {turnkey.exit_code}"
        )
    for what, left, right in [
        ("status", bundled.statuses, turnkey.statuses),
        ("action digest", bundled.digests, turnkey.digests),
    ]:
        for name in sorted(left.keys() | right.keys()):
            if left.get(name) != right.get(name):
                problems.append(
                    f"{what} of {name}: bundled {left.get(name)}, turnkey {right.get(name)}"
                )
    if not bundled.statuses:
        problems.append("no test results recorded: nothing was compared")
    return problems


def main() -> int:
    patterns = sys.argv[1:] or ["//..."]
    # Bring generated files and cells up to date once, as tk test would.
    subprocess.run(["tk", "sync"], stdout=subprocess.DEVNULL, check=True)
    failed = False
    for scenario, runner_args in SCENARIOS.items():
        bundled = run(patterns, BUNDLED_RUNNER, runner_args)
        turnkey = run(patterns, TURNKEY_RUNNER, runner_args)
        problems = compare(bundled, turnkey)
        verdict = "differs" if problems else "matches"
        print(
            f"{scenario}: {verdict} "
            f"({len(bundled.statuses)} targets, exit {bundled.exit_code})"
        )
        for problem in problems:
            print(f"  {problem}")
        failed = failed or bool(problems)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
