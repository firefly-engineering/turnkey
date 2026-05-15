"""Bazel-specific native library generator.

Generates Bazel rules for pre-compiled native libraries:
- cc_import: Imports a pre-compiled static library

Example output:
    cc_import(
        name = "ring_core_0_17_14__",
        static_library = "out_dir/libring_core_0_17_14__.a",
        visibility = ["//visibility:public"],
    )

Note: This is a proof-of-concept implementation. Full Bazel support
may require additional rules for proper Rust integration.
"""

from .native_library import NativeLibrarySpec, NativeLibraryGenerator, GeneratedRules


class BazelNativeLibraryGenerator:
    """Bazel implementation of NativeLibraryGenerator."""

    @property
    def name(self) -> str:
        return "bazel"

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        """Generate Bazel rules for a native library.

        Creates cc_import rules that can be depended on by rust_library targets.
        """
        lines = [
            "# Native library pre-compiled in Nix",
            "cc_import(",
            f'    name = "{spec.lib_name}",',
            f'    static_library = "{spec.static_lib_path}",',
            '    visibility = ["//visibility:public"],',
            ")",
            "",
        ]

        return GeneratedRules(
            rules_content="\n".join(lines),
            rules_to_load=["cc_import"],
            extra_deps=[f":{spec.lib_name}"],
            extra_rustc_flags=[f"-Lnative={spec.link_search_path}"],
        )


# Singleton instance for convenience
bazel_generator = BazelNativeLibraryGenerator()
