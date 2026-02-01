"""Build system abstraction layer.

This module provides abstractions for generating build rules across different
build systems (Buck2, Bazel, etc.). The goal is to keep dependency specifications
build-system-agnostic while supporting multiple targets.

Key abstractions:
- NativeLibrarySpec: Generic specification for pre-compiled native libraries
- NativeLibraryGenerator: Protocol for generating build-system-specific rules
"""

from .native_library import NativeLibrarySpec, NativeLibraryGenerator, GeneratedRules

__all__ = ["NativeLibrarySpec", "NativeLibraryGenerator", "GeneratedRules"]
