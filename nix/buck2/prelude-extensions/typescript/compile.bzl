# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Compiling TypeScript against npm dependencies from the jsdeps cell."""

load("@prelude//utils:utils.bzl", "flatten")
load(":npm.bzl", "npm_node_modules")

# Arguments: tsc, the output directory, the node_modules to compile against,
# the link to it to leave in the output ("" for none), then the sources
# (and `--project <tsconfig>`).
#
# It compiles inside a temporary directory, next to a link to node_modules:
# tsc looks for packages in a node_modules above the sources, and the
# action's working directory (the project root) is shared with every
# compile running at the same time. The sources (and a tsconfig) are copied
# in at their relative paths, so tsc lays the output out as it would have;
# copies, not symlinks, since tsc resolves modules from a symlink's target.
# Each package in node_modules resolves its own dependencies from its
# realpath, its instance's output in the jsdeps cell.
_SCRIPT = """#!/usr/bin/env bash
set -euo pipefail

TSC="$1"
OUT_DIR="$2"
NODE_MODULES="$3"
OUTPUT_LINK="$4"
shift 4

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# A relative path, made absolute before leaving the working directory; a
# bare command name stays one
abs() { if [[ "$1" == */* && "$1" != /* ]]; then echo "$PWD/$1"; else echo "$1"; fi; }
TSC=$(abs "$TSC")
ln -s "$(abs "$NODE_MODULES")" "$WORK_DIR/node_modules"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
for arg in "$@"; do
    if [[ -f "$arg" && "$arg" != /* ]]; then
        mkdir -p "$WORK_DIR/$(dirname "$arg")"
        cp "$arg" "$WORK_DIR/$arg"
    fi
done
cd "$WORK_DIR"

"$TSC" {flags} "$@"

# A binary's output resolves its packages from node_modules beside it
if [[ -n "$OUTPUT_LINK" ]]; then
    ln -s "$OUTPUT_LINK" "$OUT_DIR/node_modules"
fi
"""

def compile_with_npm_deps(
        ctx: AnalysisContext,
        tsc: RunInfo,
        tsc_flags: list[str],
        out_dir: Artifact,
        hidden: list,
        link_node_modules: bool) -> (Artifact, cmd_args):
    """Compiles ctx.attrs.srcs (with ctx.attrs.tsconfig) into out_dir against
    a node_modules of ctx.attrs.npm_deps. tsc_flags may name the output
    directory as $OUT_DIR. With link_node_modules, out_dir gets a
    node_modules link beside the compiled code, for node to run it with.
    Returns node_modules and the hidden inputs anything reading it needs: the
    instance closure."""
    node_modules, closure = npm_node_modules(ctx, ctx.attrs.npm_deps)
    closure_args = cmd_args(hidden = [node_modules, closure.project_as_args("artifacts")])

    script = ctx.actions.write(
        "build.sh",
        _SCRIPT.replace("{flags}", " ".join(['"{}"'.format(f) for f in tsc_flags])),
        is_executable = True,
    )

    cmd = cmd_args(script, tsc.args, out_dir.as_output(), node_modules)
    if link_node_modules:
        cmd.add(cmd_args(node_modules, relative_to = out_dir))
    else:
        cmd.add("")
    cmd.add(ctx.attrs.srcs)
    if ctx.attrs.tsconfig:
        cmd.add("--project", ctx.attrs.tsconfig)

    ctx.actions.run(
        cmd_args(cmd, closure_args, hidden = flatten([ctx.attrs.srcs, hidden])),
        category = "typescript_compile",
        identifier = ctx.label.name,
    )
    return node_modules, closure_args
