# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""The forge project a Solidity action runs in.

solidity_library and solidity_test both run forge in a scratch copy of the
repository that holds only the action's declared inputs, each at its
repository-relative path:

- the root foundry.toml and remappings.txt (the `foundry_toml` and
  `remappings_txt` attrs), so forge reads the settings and remappings
  native forge reads;
- the Solidity sources, so imports resolve as they do under native forge;
- the soldeps bundle, at the cell link's path (`soldeps_dir`), where the root
  remappings.txt targets point.

A file is copied: forge finds the sources of `src` and `test` by walking
them, and the walk skips symlinked files, so a linked test would silently not
run. A directory, the soldeps bundle, is linked, however its own files are
linked: forge reaches those through imports, reads them itself and hands
their contents to solc as standard JSON, so solc never checks a symlinked
path against its allowed directories. Either way, the source names forge
records are the staged, repository-relative ones.

forge runs with the toolchain's solc (`--use`), `--offline`, and none of the
caller's FOUNDRY_* or DAPP_* variables or ~/.foundry config: the declared
inputs are all it reads.
"""

load(":providers.bzl", "SolidityLibraryInfo", "SolidityToolchainInfo")

# The script every Solidity forge action runs:
#
#   forge.sh <forge> <solc> <out dir or ""> <path> <artifact>... -- <forge args>...
#
# It puts each artifact at <path> under $WORK_DIR (a file copied, a directory
# linked), then runs forge there with the remaining arguments. Artifact paths
# are relative to the working directory in an action and absolute under
# `buck2 run`. Without an out dir, forge's artifacts are thrown away.
_FORGE_SCRIPT = """#!/usr/bin/env bash
set -euo pipefail

FORGE="$1"
SOLC="$2"
OUT_DIR="$3"
shift 3

EXEC_ROOT="$PWD"
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

absolute() {
    case "$1" in
        /*) echo "$1" ;;
        *) echo "$EXEC_ROOT/$1" ;;
    esac
}

while [[ "$1" != "--" ]]; do
    input=$(absolute "$2")
    mkdir -p "$WORK_DIR/$(dirname "$1")"
    if [[ -d "$input" ]]; then
        ln -s "$input" "$WORK_DIR/$1"
    else
        cp "$input" "$WORK_DIR/$1"
    fi
    shift 2
done
shift

if [[ -n "$OUT_DIR" ]]; then
    OUT_DIR=$(absolute "$OUT_DIR")
    mkdir -p "$OUT_DIR"
else
    OUT_DIR="$WORK_DIR/.out"
fi

# forge reads only the staged project: no inherited configuration from the
# environment (FOUNDRY_PROFILE, FOUNDRY_SOLC, ...) or from ~/.foundry
for var in "${!FOUNDRY_@}" "${!DAPP_@}"; do
    unset "$var"
done
export HOME="$WORK_DIR/.home"

cd "$WORK_DIR"
# --use pins the toolchain's solc and --offline stops forge from resolving
# or downloading any other compiler, so the compiler is part of the command.
"$FORGE" "$@" \\
    --out "$OUT_DIR" \\
    --cache-path "$WORK_DIR/.cache" \\
    --use "$SOLC" \\
    --offline
"""

def forge_config_attrs() -> dict:
    """The attrs that give a Solidity rule its forge project.

    The macros in solidity.bzl set them from .buckconfig's [solidity], which
    turnkey generates.
    """
    return {
        "foundry_toml": attrs.dep(
            doc = "The repository's root foundry.toml (an export_file), staged at the root of forge's project.",
        ),
        "remappings_txt": attrs.option(
            attrs.dep(),
            default = None,
            doc = "The generated root remappings.txt (an export_file), staged next to foundry.toml. Required with `soldeps`.",
        ),
        "soldeps": attrs.option(
            attrs.dep(),
            default = None,
            doc = "The soldeps cell's bundle (every vendor package).",
        ),
        "soldeps_dir": attrs.option(
            attrs.string(),
            default = None,
            doc = "Where the soldeps bundle is staged, relative to the repository root: the soldeps cell link, which the root remappings.txt targets. Required with `soldeps`.",
        ),
    }

def _repo_path(package: str, name: str) -> str:
    """The repository-relative path of `name` in a root-cell `package`."""
    return package + "/" + name if package else name

def _root_file(ctx: AnalysisContext, attr: str, name: str) -> Artifact:
    """The file a root export_file attr gives, checked to sit at the root."""
    dep = getattr(ctx.attrs, attr)
    if dep.label.package:
        fail("{}: {} must be the repository root's {}, not {}".format(ctx.label, attr, name, dep.label))
    return dep[DefaultInfo].default_outputs[0]

def _staged_config(ctx: AnalysisContext) -> dict[str, Artifact]:
    """foundry.toml, and with a soldeps bundle remappings.txt and the bundle."""
    staged = {"foundry.toml": _root_file(ctx, "foundry_toml", "foundry.toml")}
    if ctx.attrs.soldeps:
        if not ctx.attrs.remappings_txt or not ctx.attrs.soldeps_dir:
            fail("{}: soldeps needs remappings_txt and soldeps_dir, the root remappings.txt targets it".format(ctx.label))
        staged["remappings.txt"] = _root_file(ctx, "remappings_txt", "remappings.txt")
        staged[ctx.attrs.soldeps_dir] = ctx.attrs.soldeps[DefaultInfo].default_outputs[0]
    elif ctx.attrs.remappings_txt:
        staged["remappings.txt"] = _root_file(ctx, "remappings_txt", "remappings.txt")
    return staged

def solidity_inputs(ctx: AnalysisContext):
    """The sources of a Solidity target, by repository-relative path.

    - `srcs`: the target's own `srcs`. A source's short path is relative to
      its package, which the root cell's packages make repository-relative.
    - `transitive_srcs`: those plus every solidity_library dep's, transitively.
    - `hidden`: the outputs of other deps, e.g. soldeps packages, whose
      sources reach forge through the staged bundle but stay declared inputs.
    """
    srcs = {_repo_path(ctx.label.package, src.short_path): src for src in ctx.attrs.srcs}
    transitive_srcs = dict(srcs)
    hidden = []
    for dep in ctx.attrs.deps:
        if SolidityLibraryInfo in dep:
            for path, src in dep[SolidityLibraryInfo].transitive_srcs.items():
                transitive_srcs.setdefault(path, src)
        elif DefaultInfo in dep:
            hidden.extend(dep[DefaultInfo].default_outputs)
    return struct(srcs = srcs, transitive_srcs = transitive_srcs, hidden = hidden)

def forge_command(
        ctx: AnalysisContext,
        inputs,
        forge_args: list,
        out_dir: OutputArtifact | None = None) -> cmd_args:
    """`forge <forge_args>` in the target's staged project (_FORGE_SCRIPT).

    `out_dir` receives forge's artifacts; without it they are thrown away.
    """
    toolchain = ctx.attrs._solidity_toolchain[SolidityToolchainInfo]
    if not toolchain.forge:
        fail("Solidity toolchain does not have forge configured. Required for {}.".format(ctx.label))

    script = ctx.actions.write("forge.sh", _FORGE_SCRIPT, is_executable = True)

    staged = _staged_config(ctx) | inputs.transitive_srcs
    stage_args = []
    for path, artifact in staged.items():
        stage_args.extend([path, artifact])

    return cmd_args(
        script,
        toolchain.forge.args,
        toolchain.solc.args,
        out_dir if out_dir else "",
        stage_args,
        "--",
        forge_args,
        hidden = inputs.hidden,
    )
