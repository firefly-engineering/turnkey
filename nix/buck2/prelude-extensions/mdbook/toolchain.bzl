# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""mdbook toolchain definition for Buck2."""

load(":providers.bzl", "MdbookToolchainInfo")

def _system_mdbook_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of system_mdbook_toolchain rule.

    Creates an mdbook toolchain from a system-provided mdbook binary.
    Path is typically provided by Nix via explicit attributes.
    """
    mdbook_path = ctx.attrs.mdbook_path

    # Create RunInfo for mdbook
    mdbook_run_info = RunInfo(args = cmd_args(mdbook_path))

    toolchain_info = MdbookToolchainInfo(
        mdbook = mdbook_run_info,
        python_path = ctx.attrs.python_path,
        serve_output_dir = ctx.attrs.serve_output_dir,
        preprocessor_paths = ctx.attrs.preprocessor_paths,
    )

    return [
        DefaultInfo(),
        toolchain_info,
    ]

system_mdbook_toolchain = rule(
    impl = _system_mdbook_toolchain_impl,
    attrs = {
        "mdbook_path": attrs.string(
            doc = "Path to the mdbook binary",
        ),
        "python_path": attrs.string(
            doc = "Path to the python3 binary",
        ),
        "serve_output_dir": attrs.option(
            attrs.string(),
            default = None,
            doc = "Directory for serve output relative to project root (e.g. '.turnkey/books'). If not set, outputs to book/ in the source directory.",
        ),
        "preprocessor_paths": attrs.list(
            attrs.string(),
            default = [],
            doc = "Directories containing mdbook preprocessor binaries (added to PATH during build/serve).",
        ),
    },
    is_toolchain_rule = True,
    doc = "Defines an mdbook toolchain using a system-provided binary.",
)
