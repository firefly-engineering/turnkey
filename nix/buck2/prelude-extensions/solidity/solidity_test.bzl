# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity test rule implementation using Foundry's forge."""

load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo", "merge_remappings")

def _solidity_test_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_test rule.

    Runs Solidity tests using Foundry's forge test command.
    Remappings are auto-generated from the toolchain's soldeps path.
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

    # Merge remappings from deps and explicit remappings
    all_remappings = merge_remappings(ctx.attrs.remappings, dep_infos)

    # Create test script that sets up forge project structure
    test_script = ctx.actions.declare_output("run_tests.sh")

    # Build explicit remappings for embedding (non-Buck-target style only)
    remappings_lines = []
    for prefix, target in all_remappings.items():
        if not target.startswith("//") and not target.startswith("@") and ":" not in target:
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

# Parse arguments
TEST_SRCS=()
DEP_SRCS=()
SOLDEPS_CELL=""
MODE="none"

for arg in "$@"; do
    if [[ "$arg" == "--test-srcs" ]]; then
        MODE="test"
        continue
    fi
    if [[ "$arg" == "--dep-srcs" ]]; then
        MODE="dep"
        continue
    fi
    if [[ "$arg" == "--soldeps-cell" ]]; then
        MODE="soldeps"
        continue
    fi
    case "$MODE" in
        test) TEST_SRCS+=("$arg") ;;
        dep) DEP_SRCS+=("$arg") ;;
        soldeps) SOLDEPS_CELL="$arg"; MODE="none" ;;
    esac
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

# Build remappings.txt - auto-generate from soldeps cell if available
REMAPPINGS_CONTENT=""

# Auto-generate remappings from soldeps cell's remappings.txt
if [[ -n "$SOLDEPS_CELL" && -f "$SOLDEPS_CELL/remappings.txt" ]]; then
    while IFS= read -r line; do
        if [[ -n "$line" && ! "$line" =~ ^# ]]; then
            # Convert relative path in remapping to absolute path
            # Format: prefix=vendor/package/ -> prefix=/abs/path/to/cell/vendor/package/
            PREFIX="${line%%=*}"
            RELPATH="${line#*=}"
            ABSPATH="$(realpath "$SOLDEPS_CELL")/$RELPATH"
            REMAPPINGS_CONTENT+="${PREFIX}=${ABSPATH}"$'\\n'
        fi
    done < "$SOLDEPS_CELL/remappings.txt"
fi

# Add any additional explicit remappings
EXPLICIT_REMAPPINGS='""" + remappings_content + """'
if [[ -n "$EXPLICIT_REMAPPINGS" ]]; then
    REMAPPINGS_CONTENT+="$EXPLICIT_REMAPPINGS"
fi

# Write remappings.txt
echo -e "$REMAPPINGS_CONTENT" > "$WORK_DIR/remappings.txt"

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

    # Add soldeps cell path from toolchain for auto-remapping
    if toolchain.soldeps_path:
        test_cmd.add("--soldeps-cell")
        test_cmd.add(toolchain.soldeps_path)

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
            doc = "Additional import remappings. Usually not needed as remappings are auto-generated from the toolchain's soldeps.",
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
            default = "toolchains//:solc",
            providers = [SolidityToolchainInfo],
        ),
    },
    doc = "Runs Solidity tests using Foundry's forge test.",
)
