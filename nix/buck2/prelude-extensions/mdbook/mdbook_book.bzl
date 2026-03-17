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

    # Build PATH additions for preprocessors
    preprocessor_paths = toolchain.preprocessor_paths or []
    path_setup = []
    if preprocessor_paths:
        path_additions = ":".join(preprocessor_paths)
        path_setup = [
            "# Add preprocessor directories to PATH",
            'export PATH="{}:$PATH"'.format(path_additions),
            "",
        ]

    # Build script content
    script_lines = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
    ] + path_setup + [
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
        "# Determine book root from book.toml path",
        'BOOK_ROOT=$(dirname "$BOOK_TOML")',
        "",
        "# Copy source files preserving directory structure relative to book root",
        'for src in "$@"; do',
        '    # Strip the book root prefix to get the relative path',
        '    if [[ "$src" == "$BOOK_ROOT/"* ]]; then',
        '        rel_path="${src#$BOOK_ROOT/}"',
        '    elif [[ "$src" == *"/$BOOK_ROOT/"* ]]; then',
        '        rel_path="${src##*/$BOOK_ROOT/}"',
        '    else',
        '        # Fallback: use basename',
        '        rel_path="$(basename "$src")"',
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
    serve_output_dir = toolchain.serve_output_dir

    # Port selection logic: find a free port so we can print the actual URL
    # This allows running multiple mdbook instances simultaneously
    port_logic = [
        "",
        "# Find a free port unless user explicitly passed --port or -p",
        "# This allows running multiple mdbook instances simultaneously",
        'if [[ ! " $* " =~ " --port " ]] && [[ ! " $* " =~ " -p " ]]; then',
        "    # Use Python to find a free port (more reliable than parsing ss/netstat)",
        '    PORT=$("$PYTHON3" -c \'import socket; s=socket.socket(); s.bind(("", 0)); print(s.getsockname()[1]); s.close()\')',
        '    PORT_ARGS="--port $PORT"',
        '    echo "Serving at http://localhost:$PORT"',
        "else",
        '    PORT_ARGS=""',
        "fi",
    ]

    if serve_output_dir:
        # Custom output directory configured in toolchain
        serve_lines = [
            "#!/usr/bin/env bash",
            "set -euo pipefail",
            "",
        ] + path_setup + [
            "# Arguments: mdbook_path book_toml python3_path [serve args]",
            "MDBOOK=$1",
            "BOOK_TOML=$2",
            "PYTHON3=$3",
            "shift 3",
            "",
            "# Get the directory containing book.toml",
            'BOOK_DIR=$(dirname "$BOOK_TOML")',
            'BOOK_NAME=$(basename "$BOOK_DIR")',
            "",
            "# Find project root (where .buckconfig is)",
            'PROJECT_ROOT=$(pwd)',
            'while [[ "$PROJECT_ROOT" != "/" && ! -f "$PROJECT_ROOT/.buckconfig" ]]; do',
            '    PROJECT_ROOT=$(dirname "$PROJECT_ROOT")',
            "done",
            "",
            "# Output to configured directory to keep source tree clean",
            'OUTPUT_DIR="$PROJECT_ROOT/{}/$BOOK_NAME"'.format(serve_output_dir),
            'mkdir -p "$OUTPUT_DIR"',
        ] + port_logic + [
            "",
            "# Run mdbook serve from the book directory with custom output",
            'cd "$BOOK_DIR"',
            '# shellcheck disable=SC2086',
            '"$MDBOOK" serve --dest-dir "$OUTPUT_DIR" $PORT_ARGS "$@"',
        ]
    else:
        # Default behavior: output to book/ in source directory
        serve_lines = [
            "#!/usr/bin/env bash",
            "set -euo pipefail",
            "",
        ] + path_setup + [
            "# Arguments: mdbook_path book_toml python3_path [serve args]",
            "MDBOOK=$1",
            "BOOK_TOML=$2",
            "PYTHON3=$3",
            "shift 3",
            "",
            "# Get the directory containing book.toml",
            'BOOK_DIR=$(dirname "$BOOK_TOML")',
        ] + port_logic + [
            "",
            "# Run mdbook serve from the book directory",
            'cd "$BOOK_DIR"',
            '# shellcheck disable=SC2086',
            '"$MDBOOK" serve $PORT_ARGS "$@"',
        ]

    ctx.actions.write(
        serve_script,
        "\n".join(serve_lines),
        is_executable = True,
    )

    # RunInfo for serve - just needs mdbook path, book.toml location, and python3 path
    serve_cmd = cmd_args(serve_script)
    serve_cmd.add(toolchain.mdbook.args)
    serve_cmd.add(ctx.attrs.book_toml)
    serve_cmd.add(toolchain.python_path)

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
