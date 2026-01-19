# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""mdbook_book rule implementation for building mdbook documentation."""

load(":providers.bzl", "MdbookBookInfo", "MdbookToolchainInfo")

def _mdbook_book_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of mdbook_book rule.

    Builds an mdbook book from Markdown sources and produces a static HTML site.
    """
    toolchain = ctx.attrs._mdbook_toolchain[MdbookToolchainInfo]

    # Declare output directory for the built book
    out_dir = ctx.actions.declare_output("book", dir = True)

    # Create a build script that sets up the directory structure and runs mdbook
    build_script = ctx.actions.declare_output("build.sh")

    # Build script content
    script_lines = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
        "# Arguments: mdbook_path out_dir book_toml src_files...",
        "MDBOOK=$1",
        "OUT_DIR=$2",
        "BOOK_TOML=$3",
        "shift 3",
        "",
        "# Make output path absolute",
        'OUT_DIR_ABS="$(pwd)/$OUT_DIR"',
        "",
        "# Create temp build directory",
        "BUILD_DIR=$(mktemp -d)",
        'trap \'rm -rf "$BUILD_DIR"\' EXIT',
        "",
        "# Copy book.toml",
        'cp "$BOOK_TOML" "$BUILD_DIR/book.toml"',
        "",
        "# Copy source files preserving directory structure",
        'for src in "$@"; do',
        '    # Get relative path (remove leading directory components to get src/...)',
        '    rel_path="${src#*/src/}"',
        '    if [[ "$src" == *"/src/"* ]]; then',
        '        rel_path="src/$rel_path"',
        '    fi',
        '    mkdir -p "$BUILD_DIR/$(dirname "$rel_path")"',
        '    cp "$src" "$BUILD_DIR/$rel_path"',
        "done",
        "",
        "# Run mdbook build",
        'cd "$BUILD_DIR"',
        '"$MDBOOK" build --dest-dir "$OUT_DIR_ABS"',
    ]

    ctx.actions.write(
        build_script,
        "\n".join(script_lines),
        is_executable = True,
    )

    # Build command
    build_cmd = cmd_args(build_script)
    build_cmd.add(toolchain.mdbook.args)
    build_cmd.add(out_dir.as_output())
    build_cmd.add(ctx.attrs.book_toml)
    for src in ctx.attrs.srcs:
        build_cmd.add(src)

    ctx.actions.run(
        build_cmd,
        category = "mdbook_build",
        identifier = ctx.label.name,
        local_only = True,
    )

    # Create serve script for `buck2 run`
    # This serves from the source directory directly (for development)
    serve_script = ctx.actions.declare_output("serve.sh")

    # Get the book directory from book_toml path
    # book_toml is like "docs/user-manual/book.toml", we need "docs/user-manual"
    serve_lines = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
        "# Arguments: mdbook_path book_toml [serve args]",
        "MDBOOK=$1",
        "BOOK_TOML=$2",
        "shift 2",
        "",
        "# Get the directory containing book.toml",
        'BOOK_DIR=$(dirname "$BOOK_TOML")',
        "",
        "# Run mdbook serve from the book directory",
        'cd "$BOOK_DIR"',
        '"$MDBOOK" serve "$@"',
    ]

    ctx.actions.write(
        serve_script,
        "\n".join(serve_lines),
        is_executable = True,
    )

    # RunInfo for serve - just needs mdbook path and book.toml location
    serve_cmd = cmd_args(serve_script)
    serve_cmd.add(toolchain.mdbook.args)
    serve_cmd.add(ctx.attrs.book_toml)

    run_info = RunInfo(args = serve_cmd)

    book_info = MdbookBookInfo(
        output_dir = out_dir,
        book_toml = ctx.attrs.book_toml,
        src_dir = ctx.attrs.src_dir if ctx.attrs.src_dir else "src",
    )

    return [
        DefaultInfo(
            default_output = out_dir,
            sub_targets = {
                "serve": [DefaultInfo(default_output = serve_script), run_info],
            },
        ),
        run_info,
        book_info,
    ]

mdbook_book = rule(
    impl = _mdbook_book_impl,
    attrs = {
        "book_toml": attrs.source(
            doc = "The book.toml configuration file",
        ),
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "Markdown source files and other assets (e.g., src/SUMMARY.md, src/chapter_1.md)",
        ),
        "src_dir": attrs.option(
            attrs.string(),
            default = None,
            doc = "Source directory path relative to book.toml (default: src)",
        ),
        "_mdbook_toolchain": attrs.toolchain_dep(
            default = "toolchains//:mdbook",
            providers = [MdbookToolchainInfo],
        ),
    },
    doc = "Builds an mdbook book from Markdown sources.",
)
