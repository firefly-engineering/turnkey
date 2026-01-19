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

    # Collect dependency outputs
    dep_outputs = []
    for dep in ctx.attrs.deps:
        if TypeScriptLibraryInfo in dep:
            dep_info = dep[TypeScriptLibraryInfo]
            if dep_info.output_dir:
                dep_outputs.append(dep_info.output_dir)

    # Collect npm dependency artifacts
    npm_dep_artifacts = []
    for npm_dep in ctx.attrs.npm_deps:
        default_info = npm_dep[DefaultInfo]
        if default_info.default_outputs:
            for output in default_info.default_outputs:
                npm_dep_artifacts.append(output)

    # If we have npm deps, use a wrapper script to set up node_modules
    if npm_dep_artifacts:
        # Create build script
        build_script = ctx.actions.declare_output("build.sh")

        # Collect tsc flags
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

        tsc_flags_str = " ".join(['"{}"'.format(f) for f in tsc_flags])

        script_content = """#!/usr/bin/env bash
set -euo pipefail

# toolchain.tsc.args expands to: node tsc_path
NODE="$1"
TSC="$2"
OUT_DIR="$3"
shift 3

# Create temporary working directory
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# Set up node_modules from npm deps
mkdir -p "$WORK_DIR/node_modules"
for npm_pkg in "$@"; do
    if [[ "$npm_pkg" == "--srcs" ]]; then
        break
    fi
    # Extract package name from package.json if it exists
    if [[ -f "$npm_pkg/package.json" ]]; then
        PKG_NAME=$(grep -o '"name"[[:space:]]*:[[:space:]]*"[^"]*"' "$npm_pkg/package.json" | head -1 | sed 's/.*"name"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/')
        if [[ -n "$PKG_NAME" ]]; then
            # Handle scoped packages (@scope/name)
            if [[ "$PKG_NAME" == @* ]]; then
                SCOPE_DIR="$WORK_DIR/node_modules/${PKG_NAME%/*}"
                mkdir -p "$SCOPE_DIR"
            fi
            # Use absolute path for symlink to work from any location
            ABS_PKG=$(cd "$(dirname "$npm_pkg")" && pwd)/$(basename "$npm_pkg")
            ln -s "$ABS_PKG" "$WORK_DIR/node_modules/$PKG_NAME"
        fi
    fi
done

# Skip npm deps and get source files
SRCS=()
FOUND_SRCS=0
for arg in "$@"; do
    if [[ "$FOUND_SRCS" == "1" ]]; then
        SRCS+=("$arg")
    elif [[ "$arg" == "--srcs" ]]; then
        FOUND_SRCS=1
    fi
done

# Create node_modules symlink in current directory for TypeScript module resolution
# tsc uses its own module resolution which requires node_modules in cwd or parent dirs
if [[ -e node_modules ]]; then
    echo "Warning: node_modules already exists in cwd, skipping symlink" >&2
else
    ln -s "$WORK_DIR/node_modules" node_modules
    trap 'rm -f node_modules; rm -rf "$WORK_DIR"' EXIT
fi

# Run tsc with node_modules available
"$NODE" "$TSC" """ + tsc_flags_str + """ "${SRCS[@]}"
"""

        ctx.actions.write(
            build_script,
            script_content,
            is_executable = True,
        )

        # Build command: script tsc out_dir npm_deps... --srcs srcs...
        build_cmd = cmd_args(build_script)
        build_cmd.add(toolchain.tsc.args)
        build_cmd.add(out_dir.as_output())

        for artifact in npm_dep_artifacts:
            build_cmd.add(artifact)

        build_cmd.add("--srcs")
        for src in ctx.attrs.srcs:
            build_cmd.add(src)

        if ctx.attrs.tsconfig:
            build_cmd.add("--project")
            build_cmd.add(ctx.attrs.tsconfig)

        ctx.actions.run(
            cmd_args(build_cmd, hidden = flatten([ctx.attrs.srcs, dep_outputs, npm_dep_artifacts])),
            category = "typescript_compile",
            identifier = ctx.label.name,
        )
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

    # Determine the main JS file path
    # Convert main.ts to main.js
    main_ts = ctx.attrs.main.short_path
    if main_ts.endswith(".ts"):
        main_js = main_ts[:-3] + ".js"
    elif main_ts.endswith(".tsx"):
        main_js = main_ts[:-4] + ".js"
    else:
        main_js = main_ts

    # Create a run script that sets up node_modules and executes the main file
    run_script = ctx.actions.declare_output("run.sh")

    if npm_dep_artifacts:
        # Build a run script that sets up node_modules at runtime
        npm_setup_lines = []
        npm_setup_lines.append("# Set up node_modules for runtime")
        npm_setup_lines.append('WORK_DIR=$(mktemp -d)')
        npm_setup_lines.append('trap \'rm -rf "$WORK_DIR"\' EXIT')
        npm_setup_lines.append('mkdir -p "$WORK_DIR/node_modules"')

        # We'll pass npm package paths as arguments to the run script
        run_script_header = """#!/usr/bin/env bash
set -euo pipefail

NODE="$1"
DIST_DIR="$2"
MAIN_JS="$3"
shift 3

# Set up node_modules from npm deps
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
mkdir -p "$WORK_DIR/node_modules"

for npm_pkg in "$@"; do
    if [[ -f "$npm_pkg/package.json" ]]; then
        PKG_NAME=$(grep -o '"name"[[:space:]]*:[[:space:]]*"[^"]*"' "$npm_pkg/package.json" | head -1 | sed 's/.*"name"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/')
        if [[ -n "$PKG_NAME" ]]; then
            if [[ "$PKG_NAME" == @* ]]; then
                SCOPE_DIR="$WORK_DIR/node_modules/${PKG_NAME%/*}"
                mkdir -p "$SCOPE_DIR"
            fi
            ln -s "$npm_pkg" "$WORK_DIR/node_modules/$PKG_NAME"
        fi
    fi
done

# Run node with node_modules in path
NODE_PATH="$WORK_DIR/node_modules" exec "$NODE" "$DIST_DIR/$MAIN_JS"
"""
        ctx.actions.write(
            run_script,
            run_script_header,
            is_executable = True,
        )

        # RunInfo with npm deps
        run_cmd = cmd_args(run_script)
        run_cmd.add(toolchain.node.args)
        run_cmd.add(out_dir)
        run_cmd.add(main_js)
        for artifact in npm_dep_artifacts:
            run_cmd.add(artifact)

        run_info = RunInfo(args = run_cmd)
    else:
        # Simple run script without npm deps
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
        "npm_deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "npm package dependencies from jsdeps cell (e.g., //jsdeps:lodash)",
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
