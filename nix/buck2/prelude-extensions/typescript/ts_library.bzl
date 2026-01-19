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

    # Collect dependency outputs for tsc to find declarations
    dep_outputs = []
    dep_infos = []
    for dep in ctx.attrs.deps:
        if TypeScriptLibraryInfo in dep:
            dep_info = dep[TypeScriptLibraryInfo]
            dep_infos.append(dep_info)
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

        if ctx.attrs.declaration:
            tsc_flags.append("--declaration")
            tsc_flags.append("--declarationDir")
            tsc_flags.append("$OUT_DIR")

        if ctx.attrs.source_map:
            tsc_flags.append("--sourceMap")

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

        if ctx.attrs.declaration:
            tsc_cmd.add("--declaration")
            tsc_cmd.add("--declarationDir", out_dir.as_output())

        if ctx.attrs.source_map:
            tsc_cmd.add("--sourceMap")

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
        "npm_deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "npm package dependencies from jsdeps cell (e.g., //jsdeps:lodash)",
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
