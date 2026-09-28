# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity toolchain definition for Buck2."""

load(":providers.bzl", "SolidityToolchainInfo")

def _system_solidity_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of system_solidity_toolchain rule.

    Creates a Solidity toolchain from system-provided binaries.
    Paths are typically provided by Nix via environment or explicit attributes.
    """
    solc_path = ctx.attrs.solc_path
    forge_path = ctx.attrs.forge_path
    cast_path = ctx.attrs.cast_path
    anvil_path = ctx.attrs.anvil_path
    jq_path = ctx.attrs.jq_path

    # The one solc: forge compiles with it (--use), whatever foundry.toml says
    solc_run_info = RunInfo(args = cmd_args(solc_path))

    # Create RunInfo for Foundry tools
    forge_run_info = RunInfo(args = cmd_args(forge_path)) if forge_path else None
    cast_run_info = RunInfo(args = cmd_args(cast_path)) if cast_path else None
    anvil_run_info = RunInfo(args = cmd_args(anvil_path)) if anvil_path else None
    jq_run_info = RunInfo(args = cmd_args(jq_path)) if jq_path else None

    toolchain_info = SolidityToolchainInfo(
        solc = solc_run_info,
        forge = forge_run_info,
        cast = cast_run_info,
        anvil = anvil_run_info,
        jq = jq_run_info,
    )

    return [
        DefaultInfo(),
        toolchain_info,
    ]

system_solidity_toolchain = rule(
    impl = _system_solidity_toolchain_impl,
    attrs = {
        "solc_path": attrs.string(
            doc = "Path to the Solidity compiler (solc) binary forge compiles with",
        ),
        "forge_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the Foundry forge binary (compiles solidity_library, runs solidity_test)",
        ),
        "cast_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the Foundry cast binary (for interactions)",
        ),
        "anvil_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the Foundry anvil binary (for local node)",
        ),
        "jq_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the jq binary (extracts solidity_contract outputs)",
        ),
    },
    is_toolchain_rule = True,
    doc = "Defines a Solidity toolchain using system-provided binaries.",
)
