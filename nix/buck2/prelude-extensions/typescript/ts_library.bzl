# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript library rule implementation."""

load("@prelude//utils:utils.bzl", "flatten")
load(":providers.bzl", "TypeScriptLibraryInfo", "TypeScriptToolchainInfo", "get_transitive_outputs")

def _typescript_library_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of typescript_library rule.

    Compiles TypeScript sources to JavaScript using tsc.
    """
    toolchain = ctx.attrs._typescript_toolchain[TypeScriptToolchainInfo]

    # Declare output directory
    out_dir = ctx.actions.declare_output("dist", dir = True)

    # Build tsc command
    tsc_cmd = cmd_args(toolchain.tsc.args)

    # Add default flags from toolchain
    for flag in toolchain.tsc_flags:
        tsc_cmd.add(flag)

    # Add output directory
    tsc_cmd.add("--outDir", out_dir.as_output())

    # Generate declaration files
    if ctx.attrs.declaration:
        tsc_cmd.add("--declaration")
        tsc_cmd.add("--declarationDir", out_dir.as_output())

    # Add source map if requested
    if ctx.attrs.source_map:
        tsc_cmd.add("--sourceMap")

    # Use provided tsconfig or generate minimal one
    if ctx.attrs.tsconfig:
        tsc_cmd.add("--project", ctx.attrs.tsconfig)
    else:
        # Default: ES modules, modern target
        tsc_cmd.add("--target", "ES2020")
        tsc_cmd.add("--module", "NodeNext")
        tsc_cmd.add("--moduleResolution", "NodeNext")
        tsc_cmd.add("--esModuleInterop")
        tsc_cmd.add("--strict")

    # Add source files
    for src in ctx.attrs.srcs:
        tsc_cmd.add(src)

    # Collect dependency outputs for tsc to find declarations
    dep_outputs = []
    dep_infos = []
    for dep in ctx.attrs.deps:
        if TypeScriptLibraryInfo in dep:
            dep_info = dep[TypeScriptLibraryInfo]
            dep_infos.append(dep_info)
            if dep_info.output_dir:
                dep_outputs.append(dep_info.output_dir)

    # Run tsc with hidden inputs for dependency tracking
    ctx.actions.run(
        cmd_args(tsc_cmd, hidden = flatten([ctx.attrs.srcs, dep_outputs])),
        category = "typescript_compile",
        identifier = ctx.label.name,
    )

    # Build transitive output set
    transitive_outputs = get_transitive_outputs(
        ctx.actions,
        value = out_dir,
        deps = dep_infos,
    )

    ts_lib_info = TypeScriptLibraryInfo(
        output_dir = out_dir,
        declaration_dir = out_dir if ctx.attrs.declaration else None,
        srcs = ctx.attrs.srcs,
        transitive_outputs = transitive_outputs,
    )

    return [
        DefaultInfo(default_output = out_dir),
        ts_lib_info,
    ]

typescript_library = rule(
    impl = _typescript_library_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "TypeScript source files to compile",
        ),
        "deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "Dependencies (other typescript_library targets)",
        ),
        "tsconfig": attrs.option(
            attrs.source(),
            default = None,
            doc = "Path to tsconfig.json (optional, uses defaults if not provided)",
        ),
        "declaration": attrs.bool(
            default = True,
            doc = "Generate .d.ts declaration files",
        ),
        "source_map": attrs.bool(
            default = False,
            doc = "Generate source maps",
        ),
        "_typescript_toolchain": attrs.toolchain_dep(
            default = "toolchains//:typescript",
            providers = [TypeScriptToolchainInfo],
        ),
    },
    doc = "Compiles TypeScript sources to JavaScript.",
)
