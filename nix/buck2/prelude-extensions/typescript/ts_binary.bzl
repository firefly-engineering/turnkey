# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript binary rule implementation."""

load("@prelude//utils:utils.bzl", "flatten")
load(":providers.bzl", "TypeScriptLibraryInfo", "TypeScriptToolchainInfo")

def _typescript_binary_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of typescript_binary rule.

    Compiles TypeScript sources and creates a runnable script.
    """
    toolchain = ctx.attrs._typescript_toolchain[TypeScriptToolchainInfo]

    # Declare output directory for compiled JS
    out_dir = ctx.actions.declare_output("dist", dir = True)

    # Build tsc command
    tsc_cmd = cmd_args(toolchain.tsc.args)

    # Add default flags from toolchain
    for flag in toolchain.tsc_flags:
        tsc_cmd.add(flag)

    # Add output directory
    tsc_cmd.add("--outDir", out_dir.as_output())

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

    # Collect dependency outputs
    dep_outputs = []
    for dep in ctx.attrs.deps:
        if TypeScriptLibraryInfo in dep:
            dep_info = dep[TypeScriptLibraryInfo]
            if dep_info.output_dir:
                dep_outputs.append(dep_info.output_dir)

    # Run tsc with hidden inputs for dependency tracking
    ctx.actions.run(
        cmd_args(tsc_cmd, hidden = flatten([ctx.attrs.srcs, dep_outputs])),
        category = "typescript_compile",
        identifier = ctx.label.name,
    )

    # Determine the main JS file path
    # Convert main.ts to main.js
    main_ts = ctx.attrs.main.short_path
    if main_ts.endswith(".ts"):
        main_js = main_ts[:-3] + ".js"
    elif main_ts.endswith(".tsx"):
        main_js = main_ts[:-4] + ".js"
    else:
        main_js = main_ts

    # Create a run script that executes the main file with node
    run_script = ctx.actions.declare_output("run.sh")
    run_script_content = cmd_args(
        "#!/bin/bash",
        "exec",
        toolchain.node.args,
        cmd_args(out_dir, format = "{}/{}".format("{}", main_js)),
        '"$@"',
        delimiter = " ",
    )

    ctx.actions.write(
        run_script,
        run_script_content,
        is_executable = True,
    )

    # Create RunInfo for `buck2 run`
    run_info = RunInfo(
        args = cmd_args(
            toolchain.node.args,
            cmd_args(out_dir, format = "{}/{}".format("{}", main_js)),
        ),
    )

    return [
        DefaultInfo(
            default_output = out_dir,
            sub_targets = {
                "run": [DefaultInfo(default_output = run_script), run_info],
            },
        ),
        run_info,
    ]

typescript_binary = rule(
    impl = _typescript_binary_impl,
    attrs = {
        "main": attrs.source(
            doc = "The main TypeScript entry point file",
        ),
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "TypeScript source files to compile",
        ),
        "deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "Dependencies (typescript_library targets)",
        ),
        "tsconfig": attrs.option(
            attrs.source(),
            default = None,
            doc = "Path to tsconfig.json",
        ),
        "_typescript_toolchain": attrs.toolchain_dep(
            default = "toolchains//:typescript",
            providers = [TypeScriptToolchainInfo],
        ),
    },
    doc = "Compiles TypeScript and creates a runnable Node.js application.",
)
