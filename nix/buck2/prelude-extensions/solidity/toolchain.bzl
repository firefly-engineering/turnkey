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

    # Create RunInfo for solc
    solc_run_info = RunInfo(args = cmd_args(solc_path))

    # Build solc_versions dict if additional versions are provided
    solc_versions = {}
    for version, path in ctx.attrs.solc_versions.items():
        solc_versions[version] = RunInfo(args = cmd_args(path))

    # Add the default solc to the versions dict if a default version is specified
    default_version = ctx.attrs.default_solc_version
    if default_version and default_version not in solc_versions:
        solc_versions[default_version] = solc_run_info

    # Create RunInfo for Foundry tools
    forge_run_info = RunInfo(args = cmd_args(forge_path)) if forge_path else None
    cast_run_info = RunInfo(args = cmd_args(cast_path)) if cast_path else None
    anvil_run_info = RunInfo(args = cmd_args(anvil_path)) if anvil_path else None

    toolchain_info = SolidityToolchainInfo(
        solc = solc_run_info,
        solc_versions = solc_versions,
        default_version = default_version,
        forge = forge_run_info,
        cast = cast_run_info,
        anvil = anvil_run_info,
        soldeps_path = ctx.attrs.soldeps_path,
    )

    return [
        DefaultInfo(),
        toolchain_info,
    ]

system_solidity_toolchain = rule(
    impl = _system_solidity_toolchain_impl,
    attrs = {
        "solc_path": attrs.string(
            doc = "Path to the default Solidity compiler (solc) binary",
        ),
        "solc_versions": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "Map of solc version strings to binary paths for multi-version support",
        ),
        "default_solc_version": attrs.option(
            attrs.string(),
            default = None,
            doc = "Default solc version to use when not specified by target",
        ),
        "forge_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the Foundry forge binary (for testing)",
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
        "soldeps_path": attrs.option(
            attrs.string(),
            default = None,
            doc = "Path to the soldeps cell directory for automatic import remapping",
        ),
    },
    is_toolchain_rule = True,
    doc = "Defines a Solidity toolchain using system-provided binaries.",
)
