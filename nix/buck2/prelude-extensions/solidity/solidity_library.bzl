# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity library rule implementation."""

load(":forge_project.bzl", "forge_command", "forge_config_attrs", "solidity_inputs")
load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo")

def _solidity_library_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_library rule.

    Compiles the sources with `forge build <srcs> --use $SOLC --offline`, in
    a forge project staged from the root foundry.toml and remappings.txt, the
    sources and the soldeps bundle (forge_project.bzl). Compiler settings and
    remappings come from those root files, as they do for native forge.
    """

    # forge's artifacts directory: <File>.sol/<Contract>.json per contract
    out_dir = ctx.actions.declare_output("artifacts", dir = True)
    inputs = solidity_inputs(ctx)

    ctx.actions.run(
        forge_command(ctx, inputs, ["build"] + inputs.srcs.keys(), out_dir.as_output()),
        category = "solidity_compile",
        identifier = ctx.label.name,
    )

    return [
        DefaultInfo(default_output = out_dir),
        SolidityLibraryInfo(
            output_dir = out_dir,
            srcs = inputs.srcs,
            transitive_srcs = inputs.transitive_srcs,
        ),
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
        "_solidity_toolchain": attrs.toolchain_dep(
            default = "toolchains//:solc",
            providers = [SolidityToolchainInfo],
        ),
    } | forge_config_attrs(),
    doc = "Compiles Solidity sources with forge, using the root foundry.toml's compiler settings.",
)
