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

    # Create a build directory that mirrors the expected mdbook structure
    build_dir = ctx.actions.declare_output("_build", dir = True)

    # Collect all source files
    all_srcs = list(ctx.attrs.srcs)
    if ctx.attrs.book_toml:
        all_srcs.append(ctx.attrs.book_toml)

    # Create a script that:
    # 1. Sets up the build directory with proper structure
    # 2. Runs mdbook build
    # 3. Copies output to the declared output directory
    build_script = ctx.actions.declare_output("build.sh")

    # Build the copy commands for source files
    # We need to recreate the directory structure under _build/src/
    copy_commands = []
    for src in ctx.attrs.srcs:
        # Get the path relative to the source root
        src_path = src.short_path
        copy_commands.append('mkdir -p "_build/$(dirname "{}")"'.format(src_path))
        copy_commands.append('cp "${{SRCS[{}]}}" "_build/{}"'.format(len(copy_commands) // 2, src_path))

    script_content = """\
#!/bin/bash
set -euo pipefail

# Create build directory structure
mkdir -p _build

# Copy book.toml
cp "$BOOK_TOML" _build/book.toml

# Copy source files preserving directory structure
{copy_commands}

# Run mdbook build
cd _build
"$MDBOOK" build --dest-dir "$OUT_DIR"
""".format(copy_commands = "\n".join(copy_commands) if copy_commands else "# No source files")

    ctx.actions.write(
        build_script,
        script_content,
        is_executable = True,
    )

    # Build the mdbook command
    build_cmd = cmd_args(
        "/bin/bash",
        build_script,
    )

    # Set environment variables for the script
    build_cmd.add(cmd_args(hidden = [
        cmd_args(toolchain.mdbook.args, format = "MDBOOK={}"),
        cmd_args(ctx.attrs.book_toml, format = "BOOK_TOML={}") if ctx.attrs.book_toml else cmd_args(),
        cmd_args(out_dir.as_output(), format = "OUT_DIR={}"),
    ]))

    # Add source files as hidden dependencies
    build_cmd.add(cmd_args(hidden = all_srcs))

    # Actually, let's use a simpler approach - create a proper action
    # that copies files and runs mdbook

    # Simpler approach: use ctx.actions.run with proper args
    run_cmd = cmd_args()
    run_cmd.add("/bin/bash")
    run_cmd.add("-c")

    # Build inline script
    inline_script = cmd_args(
        "set -euo pipefail;",
        "BUILD_DIR=$(mktemp -d);",
        "trap 'rm -rf \"$BUILD_DIR\"' EXIT;",
        cmd_args(ctx.attrs.book_toml, format = "cp {} \"$BUILD_DIR/book.toml\";"),
        "mkdir -p \"$BUILD_DIR/src\";",
        delimiter = " ",
    )

    # Copy each source file
    for src in ctx.attrs.srcs:
        inline_script.add(cmd_args(
            src,
            format = "mkdir -p \"$BUILD_DIR/$(dirname {})\" && cp {} \"$BUILD_DIR/{}\";".format(
                src.short_path, "{}", src.short_path
            ),
        ))

    # Run mdbook build
    inline_script.add(cmd_args(
        toolchain.mdbook.args,
        out_dir.as_output(),
        format = "cd \"$BUILD_DIR\" && {} build --dest-dir {};",
    ))

    run_cmd.add(inline_script)

    ctx.actions.run(
        run_cmd,
        category = "mdbook_build",
        identifier = ctx.label.name,
        local_only = True,  # mdbook may have issues with sandboxing
    )

    # Create RunInfo for `buck2 run` that serves the book
    # mdbook serve needs the source directory, not the built output
    # So we'll create a script that builds and serves
    serve_script = ctx.actions.declare_output("serve.sh")
    serve_script_content = cmd_args(
        "#!/bin/bash",
        "set -euo pipefail",
        "",
        "# Create temporary directory with book structure",
        "BUILD_DIR=$(mktemp -d)",
        'trap \'rm -rf "$BUILD_DIR"\' EXIT',
        "",
        cmd_args(ctx.attrs.book_toml, format = "cp {} \"$BUILD_DIR/book.toml\""),
        "mkdir -p \"$BUILD_DIR/src\"",
        delimiter = "\n",
    )

    # Add copy commands for source files
    for src in ctx.attrs.srcs:
        serve_script_content.add(cmd_args(
            src,
            format = "mkdir -p \"$BUILD_DIR/$(dirname {})\" && cp {} \"$BUILD_DIR/{}\"".format(
                src.short_path, "{}", src.short_path
            ),
        ))

    serve_script_content.add(cmd_args(
        "",
        "cd \"$BUILD_DIR\"",
        cmd_args(toolchain.mdbook.args, format = "{} serve \"$@\""),
        delimiter = "\n",
    ))

    ctx.actions.write(
        serve_script,
        serve_script_content,
        is_executable = True,
    )

    # RunInfo that serves the book directory
    run_info = RunInfo(
        args = cmd_args(serve_script),
    )

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
        run_info,  # Default run action is serve
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
