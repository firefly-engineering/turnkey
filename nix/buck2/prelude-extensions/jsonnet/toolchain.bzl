# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Jsonnet toolchain definition for Buck2."""

load(":providers.bzl", "JsonnetToolchainInfo")

def _system_jsonnet_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of system_jsonnet_toolchain rule.

    Creates a Jsonnet toolchain from a system-provided jsonnet binary.
    Path is typically provided by Nix via explicit attributes.
    """
    jsonnet_path = ctx.attrs.jsonnet_path

    # Create RunInfo for jsonnet
    jsonnet_run_info = RunInfo(args = cmd_args(jsonnet_path))

    toolchain_info = JsonnetToolchainInfo(
        jsonnet = jsonnet_run_info,
    )

    return [
        DefaultInfo(),
        toolchain_info,
    ]

system_jsonnet_toolchain = rule(
    impl = _system_jsonnet_toolchain_impl,
    attrs = {
        "jsonnet_path": attrs.string(
            doc = "Path to the jsonnet binary",
        ),
    },
    is_toolchain_rule = True,
    doc = "Defines a Jsonnet toolchain using a system-provided binary.",
)
