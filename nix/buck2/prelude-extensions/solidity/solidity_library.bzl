# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity library rule implementation."""

load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo", "get_transitive_srcs", "merge_remappings")

def _solidity_library_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_library rule.

    Compiles Solidity sources using solc with Standard JSON I/O.
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

    # Collect dependency info
    dep_infos = []
    dep_artifacts = []
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

    # Merge remappings from deps and explicit remappings
    all_remappings = merge_remappings(ctx.attrs.remappings, dep_infos)

    # Create build script for compilation
    build_script = ctx.actions.declare_output("compile.sh")

    # Build remappings arguments
    remappings_args = []
    for prefix, target in all_remappings.items():
        remappings_args.append('"{}"'.format("{}={}".format(prefix, target)))
    remappings_str = " ".join(remappings_args)

    # Build optimizer settings
    optimizer_args = ""
    if ctx.attrs.optimizer:
        optimizer_args = '"--optimize" "--optimize-runs" "{}"'.format(ctx.attrs.optimizer_runs)

    script_content = """#!/usr/bin/env bash
set -euo pipefail

SOLC="$1"
OUT_DIR="$2"
shift 2

# Collect source files
SRCS=()
for arg in "$@"; do
    SRCS+=("$arg")
done

# Create output directory
mkdir -p "$OUT_DIR"

# Run solc for each source file
# Using combined-json output for simplicity
"$SOLC" \\
    --combined-json abi,bin,bin-runtime,srcmap,srcmap-runtime,metadata \\
    """ + optimizer_args + """ \\
    """ + remappings_str + """ \\
    --output-dir "$OUT_DIR" \\
    --overwrite \\
    "${SRCS[@]}"

# Also generate individual contract files for convenience
"$SOLC" \\
    --abi --bin --bin-runtime --metadata \\
    """ + optimizer_args + """ \\
    """ + remappings_str + """ \\
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

    for src in ctx.attrs.srcs:
        compile_cmd.add(src)

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
            doc = "Import remappings (e.g., {'@openzeppelin/': '//soldeps:openzeppelin_contracts/'})",
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
            default = "toolchains//:solidity",
            providers = [SolidityToolchainInfo],
        ),
    },
    doc = "Compiles Solidity sources to bytecode and ABI.",
)
