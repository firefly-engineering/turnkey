"""rules.star file generation for Rust crates."""

from .generator import (
    PlatformDeps,
    PlatformNamedDeps,
    PlatformRustcFlags,
    find_matching_version,
    resolve_dep,
    extract_deps_from_section,
    get_dependencies,
    get_build_script_cfg_flags,
    generate_buck_file,
    filter_features_for_availability,
)

__all__ = [
    "PlatformDeps",
    "PlatformNamedDeps",
    "PlatformRustcFlags",
    "find_matching_version",
    "resolve_dep",
    "extract_deps_from_section",
    "get_dependencies",
    "get_build_script_cfg_flags",
    "generate_buck_file",
    "filter_features_for_availability",
]
