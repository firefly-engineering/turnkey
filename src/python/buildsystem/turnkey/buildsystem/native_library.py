"""Native library abstraction for multi-build-system support.

This module defines a build-system-agnostic specification for pre-compiled
native libraries and a protocol for generating build-system-specific rules.

The NativeLibrarySpec contains only the information needed to describe a
native library, without any build-system-specific details:
- lib_name: The library target name
- static_lib_path: Path to the compiled static library (.a file)
- link_search_path: Where rustc should search for the library

Build-system-specific generators consume this spec and produce the
appropriate rules for their target build system.
"""

from dataclasses import dataclass, field
from typing import Protocol


@dataclass
class NativeLibrarySpec:
    """Build-system-agnostic specification for a native library.

    This represents the minimal information needed to link a pre-compiled
    native library into a Rust crate, regardless of build system.

    Attributes:
        lib_name: The library name used for the build target.
                  For ring, this is "ring_core_0_17_14__" (version-specific).
        static_lib_path: Relative path to the .a file within the crate.
                         For ring, this is "out_dir/libring_core_0_17_14__.a".
        link_search_path: Directory rustc should search for the library.
                          Defaults to "out_dir".
    """

    lib_name: str
    static_lib_path: str
    link_search_path: str = "out_dir"

    @classmethod
    def from_dict(cls, d: dict) -> "NativeLibrarySpec":
        """Create a NativeLibrarySpec from a dictionary.

        This is the format used by the Nix nativeLibraries registry.
        """
        return cls(
            lib_name=d["lib_name"],
            static_lib_path=d["static_lib_path"],
            link_search_path=d.get("link_search_path", "out_dir"),
        )


@dataclass
class GeneratedRules:
    """Result of generating native library rules for a build system.

    Attributes:
        rules_content: The generated rule definitions as a string.
                       This is inserted before the main crate rule.
        rules_to_load: List of rule names to load (e.g., ["prebuilt_cxx_library"]).
        extra_deps: Additional dependencies to add to the crate.
        extra_rustc_flags: Additional rustc flags for linking.
    """

    rules_content: str = ""
    rules_to_load: list[str] = field(default_factory=list)
    extra_deps: list[str] = field(default_factory=list)
    extra_rustc_flags: list[str] = field(default_factory=list)


class NativeLibraryGenerator(Protocol):
    """Protocol for generating native library rules for a specific build system.

    Implementations of this protocol handle the build-system-specific details
    of exposing and linking native libraries.

    Examples:
        - Buck2: Generates export_file + prebuilt_cxx_library rules
        - Bazel: Generates cc_import rules
    """

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        """Generate build rules for a native library.

        Args:
            spec: The native library specification.

        Returns:
            GeneratedRules containing the build system rules and metadata.
        """
        ...

    @property
    def name(self) -> str:
        """The name of this build system (e.g., 'buck2', 'bazel')."""
        ...
