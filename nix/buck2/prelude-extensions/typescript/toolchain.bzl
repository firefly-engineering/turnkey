# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript toolchain definition for Buck2."""

load(":providers.bzl", "TypeScriptToolchainInfo")

def _system_typescript_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of system_typescript_toolchain rule.

    Creates a TypeScript toolchain from system-provided Node.js and tsc binaries.
    Paths are typically provided by Nix via environment or explicit attributes.
    """
    node_path = ctx.attrs.node_path
    tsc_path = ctx.attrs.tsc_path

    # Create RunInfo for node
    node_run_info = RunInfo(args = cmd_args(node_path))

    # Create RunInfo for tsc (runs via node)
    # tsc is typically a JS script, so we run it with node
    tsc_run_info = RunInfo(args = cmd_args(node_path, tsc_path))

    toolchain_info = TypeScriptToolchainInfo(
        node = node_run_info,
        tsc = tsc_run_info,
        tsc_flags = ctx.attrs.tsc_flags,
    )

    return [
        DefaultInfo(),
        toolchain_info,
    ]

system_typescript_toolchain = rule(
    impl = _system_typescript_toolchain_impl,
    attrs = {
        "node_path": attrs.string(
            doc = "Path to the Node.js binary",
        ),
        "tsc_path": attrs.string(
            doc = "Path to the TypeScript compiler (tsc) script",
        ),
        "tsc_flags": attrs.list(
            attrs.string(),
            default = [],
            doc = "Default flags to pass to tsc",
        ),
    },
    doc = "Defines a TypeScript toolchain using system-provided binaries.",
)
