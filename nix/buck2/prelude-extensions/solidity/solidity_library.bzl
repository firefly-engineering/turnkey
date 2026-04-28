# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity library rule implementation."""

load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo", "get_transitive_srcs", "merge_remappings")

def _solidity_library_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_library rule.

    Compiles Solidity sources using solc with automatic remapping generation.
    Remappings are auto-generated from soldeps dependencies - no manual config needed.
    """
    toolchain = ctx.attrs._solidity_toolchain[SolidityToolchainInfo]

    # Determine which solc to use
    solc_version = ctx.attrs.solc_version or toolchain.default_version
    if solc_version and solc_version in toolchain.solc_versions:
        solc = toolchain.solc_versions[solc_version]
    else:
        solc = toolchain.solc

    # Declare output directory for compiled artifacts
    out_dir = ctx.actions.declare_output("artifacts", dir = True)

    # Collect dependency info and artifacts
    dep_infos = []
    dep_artifacts = []
    soldeps_dirs = []  # Track soldeps cell directories for auto-remapping

    for dep in ctx.attrs.deps:
        if SolidityLibraryInfo in dep:
            dep_info = dep[SolidityLibraryInfo]
            dep_infos.append(dep_info)
            if dep_info.output_dir:
                dep_artifacts.append(dep_info.output_dir)
        elif DefaultInfo in dep:
            # Handle filegroup dependencies (e.g., from soldeps)
            default_info = dep[DefaultInfo]
            if default_info.default_outputs:
                for output in default_info.default_outputs:
                    dep_artifacts.append(output)
                    # Track if this looks like a soldeps dependency
                    # The output will be the filegroup's files, but we need the cell root
                    soldeps_dirs.append(output)

    # Merge remappings from deps and explicit remappings
    all_remappings = merge_remappings(ctx.attrs.remappings, dep_infos)

    # Create build script for compilation
    build_script = ctx.actions.declare_output("compile.sh")

    # Build optimizer settings
    optimizer_args = ""
    if ctx.attrs.optimizer:
        optimizer_args = '"--optimize" "--optimize-runs" "{}"'.format(ctx.attrs.optimizer_runs)

    # The script auto-generates remappings from the soldeps remappings.txt
    script_content = """#!/usr/bin/env bash
set -euo pipefail

SOLC="$1"
OUT_DIR="$2"
shift 2

# Parse args - sources come first, then --soldeps-cell <path>, then --remappings <extra>
SRCS=()
SOLDEPS_CELL=""
EXTRA_REMAPPINGS=()
MODE="srcs"

for arg in "$@"; do
    if [[ "$arg" == "--soldeps-cell" ]]; then
        MODE="soldeps"
        continue
    fi
    if [[ "$arg" == "--remappings" ]]; then
        MODE="remappings"
        continue
    fi
    if [[ "$MODE" == "srcs" ]]; then
        SRCS+=("$arg")
    elif [[ "$MODE" == "soldeps" ]]; then
        SOLDEPS_CELL="$arg"
        MODE="srcs"  # Reset after reading the cell path
    elif [[ "$MODE" == "remappings" ]]; then
        EXTRA_REMAPPINGS+=("$arg")
    fi
done

# Create output directory
mkdir -p "$OUT_DIR"

# Build remapping flags
REMAPPING_FLAGS=()

# Auto-generate remappings from soldeps cell's remappings.txt
if [[ -n "$SOLDEPS_CELL" && -f "$SOLDEPS_CELL/remappings.txt" ]]; then
    while IFS= read -r line; do
        if [[ -n "$line" && ! "$line" =~ ^# ]]; then
            # Convert relative path in remapping to absolute path
            # Format: prefix=vendor/package/ -> prefix=/abs/path/to/cell/vendor/package/
            PREFIX="${line%%=*}"
            RELPATH="${line#*=}"
            ABSPATH="$(realpath "$SOLDEPS_CELL")/$RELPATH"
            REMAPPING_FLAGS+=("${PREFIX}=${ABSPATH}")
        fi
    done < "$SOLDEPS_CELL/remappings.txt"
fi

# Add any extra explicit remappings
for remap in "${EXTRA_REMAPPINGS[@]}"; do
    REMAPPING_FLAGS+=("$remap")
done

# Run solc with combined-json output
"$SOLC" \\
    --combined-json abi,bin,bin-runtime,srcmap,srcmap-runtime,metadata \\
    """ + optimizer_args + """ \\
    "${REMAPPING_FLAGS[@]}" \\
    --output-dir "$OUT_DIR" \\
    --overwrite \\
    "${SRCS[@]}"

# Also generate individual contract files
"$SOLC" \\
    --abi --bin --bin-runtime --metadata \\
    """ + optimizer_args + """ \\
    "${REMAPPING_FLAGS[@]}" \\
    --output-dir "$OUT_DIR" \\
    --overwrite \\
    "${SRCS[@]}"
"""

    ctx.actions.write(
        build_script,
        script_content,
        is_executable = True,
    )

    # Build command
    compile_cmd = cmd_args(build_script)
    compile_cmd.add(solc.args)
    compile_cmd.add(out_dir.as_output())

    # Add source files
    for src in ctx.attrs.srcs:
        compile_cmd.add(src)

    # Add soldeps cell path for auto-remapping (resolved from .buckconfig)
    soldeps_cell_path = read_root_config("cells", "soldeps", None)
    if soldeps_cell_path:
        compile_cmd.add("--soldeps-cell")
        compile_cmd.add(soldeps_cell_path)

    # Add explicit remappings (these should be path-based, not Buck targets)
    if all_remappings:
        compile_cmd.add("--remappings")
        for prefix, target in all_remappings.items():
            # Skip Buck target-style remappings (they should use deps instead)
            if not target.startswith("//") and not target.startswith("@") and ":" not in target:
                compile_cmd.add("{}={}".format(prefix, target))

    ctx.actions.run(
        cmd_args(compile_cmd, hidden = ctx.attrs.srcs + dep_artifacts),
        category = "solidity_compile",
        identifier = ctx.label.name,
    )

    # Build transitive sources set
    transitive_srcs = get_transitive_srcs(
        ctx.actions,
        deps = dep_infos,
    )

    sol_lib_info = SolidityLibraryInfo(
        output_dir = out_dir,
        srcs = ctx.attrs.srcs,
        remappings = ctx.attrs.remappings,
        solc_version = solc_version,
        transitive_srcs = transitive_srcs,
        transitive_remappings = all_remappings,
    )

    return [
        DefaultInfo(default_output = out_dir),
        sol_lib_info,
    ]

solidity_library = rule(
    impl = _solidity_library_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "Solidity source files (.sol) to compile",
        ),
        "deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "Dependencies (other solidity_library targets or filegroups from soldeps)",
        ),
        "remappings": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "Additional import remappings. Usually not needed as remappings are auto-generated from the toolchain's soldeps.",
        ),
        "solc_version": attrs.option(
            attrs.string(),
            default = None,
            doc = "Specific solc version to use (defaults to toolchain default)",
        ),
        "optimizer": attrs.bool(
            default = True,
            doc = "Enable the Solidity optimizer",
        ),
        "optimizer_runs": attrs.int(
            default = 200,
            doc = "Number of optimizer runs (higher = optimized for more frequent calls)",
        ),
        "_solidity_toolchain": attrs.toolchain_dep(
            default = "toolchains//:solc",
            providers = [SolidityToolchainInfo],
        ),
    },
    doc = "Compiles Solidity sources to bytecode and ABI. Remappings are auto-generated from the toolchain.",
)
