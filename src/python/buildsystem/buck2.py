"""Buck2-specific native library generator.

Generates Buck2 rules for pre-compiled native libraries:
- export_file: Exposes the static library file
- prebuilt_cxx_library: Wraps it as a linkable C++ library

Example output:
    export_file(
        name = "ring_core_0_17_14___file",
        src = "out_dir/libring_core_0_17_14__.a",
        visibility = ["PUBLIC"],
    )

    prebuilt_cxx_library(
        name = "ring_core_0_17_14__",
        static_lib = ":ring_core_0_17_14___file",
        link_whole = True,
        preferred_linkage = "static",
        visibility = ["PUBLIC"],
    )
"""

from .native_library import NativeLibrarySpec, NativeLibraryGenerator, GeneratedRules


class Buck2NativeLibraryGenerator:
    """Buck2 implementation of NativeLibraryGenerator."""

    @property
    def name(self) -> str:
        return "buck2"

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        """Generate Buck2 rules for a native library.

        Creates export_file and prebuilt_cxx_library rules that can be
        depended on by rust_library targets.
        """
        lines = [
            "# Native library pre-compiled in Nix",
            "export_file(",
            f'    name = "{spec.lib_name}_file",',
            f'    src = "{spec.static_lib_path}",',
            '    visibility = ["PUBLIC"],',
            ")",
            "",
            "prebuilt_cxx_library(",
            f'    name = "{spec.lib_name}",',
            f'    static_lib = ":{spec.lib_name}_file",',
            "    link_whole = True,",
            '    preferred_linkage = "static",',
            '    visibility = ["PUBLIC"],',
            ")",
            "",
        ]

        return GeneratedRules(
            rules_content="\n".join(lines),
            rules_to_load=["prebuilt_cxx_library", "export_file"],
            extra_deps=[f":{spec.lib_name}"],
            extra_rustc_flags=[f"-Lnative={spec.link_search_path}"],
        )


# Singleton instance for convenience
buck2_generator = Buck2NativeLibraryGenerator()
