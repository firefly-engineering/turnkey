# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript binary rule implementation."""

load("@prelude//test_caching:test_caching.bzl", "test_caching_kwargs")
load("@prelude//utils:utils.bzl", "flatten")
load(":compile.bzl", "compile_with_npm_deps")
load(":npm.bzl", "NpmPackageInfo")
load(":providers.bzl", "TypeScriptLibraryInfo", "TypeScriptToolchainInfo")

def _binary(ctx: AnalysisContext) -> (DefaultInfo, RunInfo):
    """Compiles TypeScript sources and creates a runnable script: the
    providers typescript_binary and typescript_test share."""
    toolchain = ctx.attrs._typescript_toolchain[TypeScriptToolchainInfo]

    # Declare output directory for compiled JS
    out_dir = ctx.actions.declare_output("dist", dir = True)

    # Collect dependency outputs
    dep_outputs = []
    for dep in ctx.attrs.deps:
        if TypeScriptLibraryInfo in dep:
            dep_info = dep[TypeScriptLibraryInfo]
            if dep_info.output_dir:
                dep_outputs.append(dep_info.output_dir)

    # With npm deps, tsc compiles next to a node_modules of their package
    # directories, and the output gets a node_modules link of its own, for
    # node to resolve them from the compiled code's realpath (compile.bzl)
    npm_hidden = []
    if ctx.attrs.npm_deps:
        tsc_flags = list(toolchain.tsc_flags)
        tsc_flags.append("--outDir")
        tsc_flags.append("$OUT_DIR")

        if not ctx.attrs.tsconfig:
            tsc_flags.extend([
                "--target", "ES2020",
                "--module", "NodeNext",
                "--moduleResolution", "NodeNext",
                "--esModuleInterop",
                "--strict",
            ])

        _, npm_hidden = compile_with_npm_deps(ctx, toolchain.tsc, tsc_flags, out_dir, dep_outputs, link_node_modules = True)
        npm_hidden = [npm_hidden]
    else:
        # No npm deps - use direct tsc invocation (original behavior)
        tsc_cmd = cmd_args(toolchain.tsc.args)

        for flag in toolchain.tsc_flags:
            tsc_cmd.add(flag)

        tsc_cmd.add("--outDir", out_dir.as_output())

        if ctx.attrs.tsconfig:
            tsc_cmd.add("--project", ctx.attrs.tsconfig)
        else:
            tsc_cmd.add("--target", "ES2020")
            tsc_cmd.add("--module", "NodeNext")
            tsc_cmd.add("--moduleResolution", "NodeNext")
            tsc_cmd.add("--esModuleInterop")
            tsc_cmd.add("--strict")

        for src in ctx.attrs.srcs:
            tsc_cmd.add(src)

        ctx.actions.run(
            cmd_args(tsc_cmd, hidden = flatten([ctx.attrs.srcs, dep_outputs])),
            category = "typescript_compile",
            identifier = ctx.label.name,
        )

    # Determine the main JS file path: tsc writes a .ts or .tsx file's
    # code to .js, a .mts file's (ESM) to .mjs and a .cts file's (CommonJS)
    # to .cjs
    main_ts = ctx.attrs.main.short_path
    main_js = main_ts
    for ext, js in [(".ts", ".js"), (".tsx", ".js"), (".mts", ".mjs"), (".cts", ".cjs")]:
        if main_ts.endswith(ext):
            main_js = main_ts[:-len(ext)] + js
            break

    # Create a run script that executes the main file. Its npm packages
    # resolve from the node_modules link in the output, which needs the
    # instance closure (npm_hidden) materialized.
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

    run_info = RunInfo(
        args = cmd_args(
            toolchain.node.args,
            cmd_args(out_dir, format = "{}/{}".format("{}", main_js)),
            hidden = npm_hidden,
        ),
    )

    default_info = DefaultInfo(
        default_output = out_dir,
        other_outputs = npm_hidden,
        sub_targets = {
            "run": [DefaultInfo(default_output = run_script, other_outputs = npm_hidden), run_info],
        },
    )
    return default_info, run_info

def _typescript_binary_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of typescript_binary rule."""
    default_info, run_info = _binary(ctx)
    return [default_info, run_info]

def _typescript_test_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of typescript_test rule: the binary, run as the test,
    which passes when it exits 0."""
    default_info, run_info = _binary(ctx)
    return [
        default_info,
        run_info,
        # Cacheable: the test reads only the compiled code and the npm
        # instances it links, all hidden inputs of its command, and runs
        # the toolchain's node.
        ExternalRunnerTestInfo(**test_caching_kwargs({
            "type": "typescript",
            "command": [run_info.args],
            "labels": ctx.attrs.labels,
        })),
    ]

_BINARY_ATTRS = {
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
    "npm_deps": attrs.list(
        attrs.dep(providers = [NpmPackageInfo]),
        default = [],
        doc = "npm packages from the jsdeps cell, by their npm name (e.g., jsdeps//:@types/lodash)",
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
}

typescript_binary = rule(
    impl = _typescript_binary_impl,
    attrs = _BINARY_ATTRS,
    doc = "Compiles TypeScript and creates a runnable Node.js application.",
)

typescript_test = rule(
    impl = _typescript_test_impl,
    attrs = _BINARY_ATTRS | {
        "labels": attrs.list(
            attrs.string(),
            default = [],
            doc = "Target labels. `no-test-cache` keeps the test out of turnkey's test result caching.",
        ),
    },
    doc = "Compiles TypeScript and runs it with Node.js as a test, which passes when it exits 0.",
)
