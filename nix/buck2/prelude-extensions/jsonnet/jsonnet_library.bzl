# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Jsonnet library rule for Buck2."""

load(":providers.bzl", "JsonnetLibraryInfo", "JsonnetToolchainInfo")

def _jsonnet_library_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of jsonnet_library rule.

    Compiles Jsonnet source files to JSON output.
    """
    toolchain = ctx.attrs._jsonnet_toolchain[JsonnetToolchainInfo]

    # Collect import paths from dependencies
    import_paths = []
    for dep in ctx.attrs.deps:
        if JsonnetLibraryInfo in dep:
            dep_info = dep[JsonnetLibraryInfo]
            if dep_info.import_paths:
                import_paths.extend(dep_info.import_paths)

    # Add source directory to import paths
    if ctx.attrs.srcs:
        # Get the directory of the first source file for imports
        src_dir = ctx.attrs.srcs[0].short_path.rsplit("/", 1)[0] if "/" in ctx.attrs.srcs[0].short_path else "."
        import_paths.append(src_dir)

    # Determine output file name
    if ctx.attrs.out:
        out_name = ctx.attrs.out
    else:
        # Default: use the name of the first source with .json extension
        src_name = ctx.attrs.srcs[0].short_path.rsplit("/", 1)[-1]
        out_name = src_name.rsplit(".", 1)[0] + ".json"

    output = ctx.actions.declare_output(out_name)

    # Collect all hidden inputs for dependency tracking
    hidden_inputs = list(ctx.attrs.srcs)
    for dep in ctx.attrs.deps:
        if JsonnetLibraryInfo in dep:
            dep_info = dep[JsonnetLibraryInfo]
            if dep_info.sources:
                hidden_inputs.extend(dep_info.sources)

    # Build the jsonnet command with hidden inputs
    cmd = cmd_args(toolchain.jsonnet, hidden = hidden_inputs)

    # Add import paths (-J flags)
    for path in import_paths:
        cmd.add("-J", path)

    # Add external string variables (--ext-str)
    for key, value in ctx.attrs.ext_strs.items():
        cmd.add("--ext-str", "{}={}".format(key, value))

    # Add external code variables (--ext-code)
    for key, value in ctx.attrs.ext_codes.items():
        cmd.add("--ext-code", "{}={}".format(key, value))

    # Add top-level arguments (--tla-str)
    for key, value in ctx.attrs.tla_strs.items():
        cmd.add("--tla-str", "{}={}".format(key, value))

    # Add top-level code arguments (--tla-code)
    for key, value in ctx.attrs.tla_codes.items():
        cmd.add("--tla-code", "{}={}".format(key, value))

    # Add source file (use the first/main source)
    cmd.add(ctx.attrs.srcs[0])

    # Add output
    cmd.add("-o", output.as_output())

    ctx.actions.run(cmd, category = "jsonnet", identifier = ctx.label.name)

    return [
        DefaultInfo(default_output = output),
        JsonnetLibraryInfo(
            output = output,
            sources = ctx.attrs.srcs,
            import_paths = import_paths,
        ),
    ]

jsonnet_library = rule(
    impl = _jsonnet_library_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            doc = "Jsonnet source files. The first file is the entry point.",
        ),
        "deps": attrs.list(
            attrs.dep(providers = [JsonnetLibraryInfo]),
            default = [],
            doc = "Dependencies on other jsonnet_library targets.",
        ),
        "out": attrs.option(
            attrs.string(),
            default = None,
            doc = "Output file name. Defaults to <first_src_basename>.json.",
        ),
        "ext_strs": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "External string variables (--ext-str key=value).",
        ),
        "ext_codes": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "External code variables (--ext-code key=value).",
        ),
        "tla_strs": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "Top-level argument strings (--tla-str key=value).",
        ),
        "tla_codes": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "Top-level argument code (--tla-code key=value).",
        ),
        "_jsonnet_toolchain": attrs.toolchain_dep(
            default = "toolchains//:jsonnet",
            providers = [JsonnetToolchainInfo],
        ),
    },
    doc = "Compiles Jsonnet source files to JSON.",
)
