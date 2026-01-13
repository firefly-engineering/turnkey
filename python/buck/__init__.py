"""BUCK file generation for Rust crates."""

from .generator import (
    find_matching_version,
    resolve_dep,
    extract_deps_from_section,
    get_dependencies,
    get_build_script_cfg_flags,
    get_native_library_info,
    generate_buck_file,
    filter_features_for_availability,
)

__all__ = [
    "find_matching_version",
    "resolve_dep",
    "extract_deps_from_section",
    "get_dependencies",
    "get_build_script_cfg_flags",
    "get_native_library_info",
    "generate_buck_file",
    "filter_features_for_availability",
]
