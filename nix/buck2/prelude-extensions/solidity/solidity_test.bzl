# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity test rule implementation using Foundry's forge."""

load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo", "merge_remappings")

def _solidity_test_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_test rule.

    Runs Solidity tests using Foundry's forge test command.
    """
    toolchain = ctx.attrs._solidity_toolchain[SolidityToolchainInfo]

    if not toolchain.forge:
        fail("Solidity toolchain does not have forge configured. Required for solidity_test.")

    # Collect dependency info and remappings
    dep_infos = []
    dep_artifacts = []
    for dep in ctx.attrs.deps:
        if SolidityLibraryInfo in dep:
            dep_info = dep[SolidityLibraryInfo]
            dep_infos.append(dep_info)
            for src in dep_info.srcs:
                dep_artifacts.append(src)
            if dep_info.output_dir:
                dep_artifacts.append(dep_info.output_dir)
        elif DefaultInfo in dep:
            default_info = dep[DefaultInfo]
            if default_info.default_outputs:
                for output in default_info.default_outputs:
                    dep_artifacts.append(output)

    # Merge remappings
    all_remappings = merge_remappings(ctx.attrs.remappings, dep_infos)

    # Create test script that sets up forge project structure
    test_script = ctx.actions.declare_output("run_tests.sh")

    # Build remappings for remappings.txt
    remappings_lines = []
    for prefix, target in all_remappings.items():
        remappings_lines.append("{}={}".format(prefix, target))
    remappings_content = "\\n".join(remappings_lines)

    # Build forge test arguments
    forge_args = []
    if ctx.attrs.verbosity > 0:
        forge_args.append("-" + "v" * ctx.attrs.verbosity)
    if ctx.attrs.fuzz_runs:
        forge_args.append("--fuzz-runs")
        forge_args.append(str(ctx.attrs.fuzz_runs))
    if ctx.attrs.fork_url:
        forge_args.append("--fork-url")
        forge_args.append(ctx.attrs.fork_url)
    if ctx.attrs.match_test:
        forge_args.append("--match-test")
        forge_args.append(ctx.attrs.match_test)
    if ctx.attrs.match_contract:
        forge_args.append("--match-contract")
        forge_args.append(ctx.attrs.match_contract)
    if ctx.attrs.gas_report:
        forge_args.append("--gas-report")

    forge_args_str = " ".join(['"{}"'.format(a) for a in forge_args]) if forge_args else ""

    script_content = """#!/usr/bin/env bash
set -euo pipefail

FORGE="$1"
shift

# Create temporary forge project structure
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$WORK_DIR/src"
mkdir -p "$WORK_DIR/test"
mkdir -p "$WORK_DIR/lib"

# Copy test sources
TEST_SRCS=()
FOUND_TESTS=0
DEP_SRCS=()
for arg in "$@"; do
    if [[ "$arg" == "--test-srcs" ]]; then
        FOUND_TESTS=1
        continue
    fi
    if [[ "$arg" == "--dep-srcs" ]]; then
        FOUND_TESTS=2
        continue
    fi
    if [[ "$FOUND_TESTS" == "1" ]]; then
        TEST_SRCS+=("$arg")
    elif [[ "$FOUND_TESTS" == "2" ]]; then
        DEP_SRCS+=("$arg")
    fi
done

# Copy test files
for src in "${TEST_SRCS[@]}"; do
    cp "$src" "$WORK_DIR/test/"
done

# Copy/link dependency sources
for src in "${DEP_SRCS[@]}"; do
    if [[ -d "$src" ]]; then
        # It's a directory (likely from soldeps), symlink it
        PKG_NAME=$(basename "$src")
        ln -s "$(realpath "$src")" "$WORK_DIR/lib/$PKG_NAME"
    elif [[ -f "$src" ]]; then
        cp "$src" "$WORK_DIR/src/"
    fi
done

# Create remappings.txt
cat > "$WORK_DIR/remappings.txt" << 'REMAPPINGS'
""" + remappings_content + """
REMAPPINGS

# Create minimal foundry.toml
cat > "$WORK_DIR/foundry.toml" << 'FOUNDRY'
[profile.default]
src = "src"
test = "test"
libs = ["lib"]
out = "out"
FOUNDRY

# Run forge test
cd "$WORK_DIR"
"$FORGE" test """ + forge_args_str + """
"""

    ctx.actions.write(
        test_script,
        script_content,
        is_executable = True,
    )

    # Build test command
    test_cmd = cmd_args(test_script)
    test_cmd.add(toolchain.forge.args)

    # Add test sources
    test_cmd.add("--test-srcs")
    for src in ctx.attrs.srcs:
        test_cmd.add(src)

    # Add dependency sources
    test_cmd.add("--dep-srcs")
    for artifact in dep_artifacts:
        test_cmd.add(artifact)

    # Create run info for test execution
    run_info = RunInfo(args = test_cmd)

    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            type = "solidity",
            command = [test_cmd],
        ),
        run_info,
    ]

solidity_test = rule(
    impl = _solidity_test_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "Solidity test source files (.t.sol)",
        ),
        "deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "Dependencies (solidity_library targets or filegroups from soldeps)",
        ),
        "remappings": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "Import remappings for test files",
        ),
        "fuzz_runs": attrs.option(
            attrs.int(),
            default = None,
            doc = "Number of fuzz test runs (default: forge's default of 256)",
        ),
        "fork_url": attrs.option(
            attrs.string(),
            default = None,
            doc = "RPC URL for forking mainnet/testnet state",
        ),
        "match_test": attrs.option(
            attrs.string(),
            default = None,
            doc = "Only run tests matching this regex pattern",
        ),
        "match_contract": attrs.option(
            attrs.string(),
            default = None,
            doc = "Only run tests in contracts matching this regex pattern",
        ),
        "gas_report": attrs.bool(
            default = False,
            doc = "Print gas usage report",
        ),
        "verbosity": attrs.int(
            default = 0,
            doc = "Verbosity level (0-5, maps to forge -v flags)",
        ),
        "_solidity_toolchain": attrs.toolchain_dep(
            default = "toolchains//:solidity",
            providers = [SolidityToolchainInfo],
        ),
    },
    doc = "Runs Solidity tests using Foundry's forge test.",
)
