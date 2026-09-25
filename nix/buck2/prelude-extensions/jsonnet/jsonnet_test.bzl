# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Jsonnet test rule for Buck2.

Supports two test modes:
1. Assertion mode (default): Compile the test file - if it contains std.assertEqual
   or std.assertMsg calls that fail, jsonnet will error and the test fails.
2. Golden file mode: Compare the compiled output against an expected JSON file.
"""

load(":providers.bzl", "JsonnetLibraryInfo", "JsonnetToolchainInfo")

def _jsonnet_test_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of jsonnet_test rule."""
    toolchain = ctx.attrs._jsonnet_toolchain[JsonnetToolchainInfo]

    # Collect import paths from dependencies
    import_paths = []
    dep_sources = []
    for dep in ctx.attrs.deps:
        if JsonnetLibraryInfo in dep:
            dep_info = dep[JsonnetLibraryInfo]
            if dep_info.import_paths:
                import_paths.extend(dep_info.import_paths)
            if dep_info.sources:
                dep_sources.extend(dep_info.sources)

    # Add source directory to import paths
    src_path = ctx.attrs.src.short_path
    src_dir = src_path.rsplit("/", 1)[0] if "/" in src_path else "."
    import_paths.append(src_dir)

    # Stage the test source and its dependencies' sources into a copied
    # directory, and run jsonnet there. Imports resolve relative to the
    # importing file and to the -J paths, so both must point inside this
    # directory: an import of a file that isn't declared then fails instead of
    # being read from the checkout. A copy, not symlinks, so a resolved path
    # can't lead back into the source tree.
    staged = {}
    for source in [ctx.attrs.src] + dep_sources:
        existing = staged.get(source.short_path)
        if existing != None and existing != source:
            fail("jsonnet_test: two sources share the path `{}`".format(source.short_path))
        staged[source.short_path] = source
    srcs_dir = ctx.actions.copied_dir("__jsonnet_srcs__", staged)

    # Create test script
    test_script = ctx.actions.declare_output("run_test.sh")

    # Build import path args for the script
    import_args = []
    for path in import_paths:
        import_args.append("-J")
        import_args.append("$SRCS/" + path)
    import_args_str = " ".join(['"{}"'.format(a) for a in import_args]) if import_args else ""

    # Build ext-str args
    ext_str_args = []
    for key, value in ctx.attrs.ext_strs.items():
        ext_str_args.append("--ext-str")
        ext_str_args.append("{}={}".format(key, value))
    ext_str_args_str = " ".join(['"{}"'.format(a) for a in ext_str_args]) if ext_str_args else ""

    # Build ext-code args
    ext_code_args = []
    for key, value in ctx.attrs.ext_codes.items():
        ext_code_args.append("--ext-code")
        ext_code_args.append("{}={}".format(key, value))
    ext_code_args_str = " ".join(['"{}"'.format(a) for a in ext_code_args]) if ext_code_args else ""

    # Determine test mode based on whether golden file is provided
    if ctx.attrs.golden:
        # Golden file mode: compare output to expected
        script_content = """#!/usr/bin/env bash
set -euo pipefail

JSONNET="$1"
SRCS="$2"
SRC="$SRCS/$3"
GOLDEN="$4"
shift 4

# Compile jsonnet to temp file
OUTPUT=$(mktemp)
trap 'rm -f "$OUTPUT"' EXIT

"$JSONNET" {import_args} {ext_str_args} {ext_code_args} "$SRC" -o "$OUTPUT"

# Compare output to golden file
if diff -q "$OUTPUT" "$GOLDEN" > /dev/null 2>&1; then
    echo "PASS: Output matches golden file"
    exit 0
else
    echo "FAIL: Output differs from golden file"
    echo ""
    echo "=== Diff (actual vs expected) ==="
    diff -u "$GOLDEN" "$OUTPUT" || true
    exit 1
fi
""".format(
            import_args = import_args_str,
            ext_str_args = ext_str_args_str,
            ext_code_args = ext_code_args_str,
        )
    else:
        # Assertion mode: just compile and check for errors
        script_content = """#!/usr/bin/env bash
set -euo pipefail

JSONNET="$1"
SRCS="$2"
SRC="$SRCS/$3"
shift 3

# Compile jsonnet - assertions will cause non-zero exit
# Redirect output to /dev/null since we only care about success/failure
if "$JSONNET" {import_args} {ext_str_args} {ext_code_args} "$SRC" > /dev/null; then
    echo "PASS: All assertions passed"
    exit 0
else
    echo "FAIL: Jsonnet compilation/assertions failed"
    exit 1
fi
""".format(
            import_args = import_args_str,
            ext_str_args = ext_str_args_str,
            ext_code_args = ext_code_args_str,
        )

    ctx.actions.write(
        test_script,
        script_content,
        is_executable = True,
    )

    # Build test command. Every jsonnet input reaches the test through the
    # staged directory.
    test_cmd = cmd_args(test_script)
    test_cmd.add(toolchain.jsonnet)
    test_cmd.add(srcs_dir)
    test_cmd.add(src_path)

    if ctx.attrs.golden:
        test_cmd.add(ctx.attrs.golden)

    # Create run info for test execution
    run_info = RunInfo(args = test_cmd)

    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            type = "jsonnet",
            command = [test_cmd],
        ),
        run_info,
    ]

jsonnet_test = rule(
    impl = _jsonnet_test_impl,
    attrs = {
        "src": attrs.source(
            doc = "Jsonnet test source file containing assertions or producing output for comparison.",
        ),
        "golden": attrs.option(
            attrs.source(),
            default = None,
            doc = "Golden file to compare output against. If not provided, test passes if compilation succeeds (assertion mode).",
        ),
        "deps": attrs.list(
            attrs.dep(providers = [JsonnetLibraryInfo]),
            default = [],
            doc = "Dependencies on jsonnet_library targets.",
        ),
        "ext_strs": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "External string variables (--ext-str key=value).",
        ),
        "ext_codes": attrs.dict(
            key = attrs.string(),
            value = attrs.string(),
            default = {},
            doc = "External code variables (--ext-code key=value).",
        ),
        "_jsonnet_toolchain": attrs.toolchain_dep(
            default = "toolchains//:jsonnet",
            providers = [JsonnetToolchainInfo],
        ),
    },
    doc = """Tests Jsonnet files using assertions or golden file comparison.

Two test modes are supported:

1. Assertion mode (default): The test file is compiled. If it contains
   std.assertEqual or std.assertMsg calls that fail, the test fails.

   Example test file:
   ```jsonnet
   local lib = import 'mylib.libsonnet';
   std.assertEqual(lib.add(1, 2), 3) &&
   std.assertEqual(lib.multiply(2, 3), 6)
   ```

2. Golden file mode: The test file is compiled and the output is compared
   against a golden file. The test passes if they match exactly.

   ```starlark
   jsonnet_test(
       name = "config_test",
       src = "config.jsonnet",
       golden = "config.expected.json",
   )
   ```
""",
)
