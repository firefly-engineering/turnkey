"""rules.star file generation for Rust crates."""

from .generator import (
    PlatformDeps,
    PlatformNamedDeps,
    PlatformRustcFlags,
    CrateFixup,
    find_matching_version,
    resolve_dep,
    extract_deps_from_section,
    get_dependencies,
    get_fixup_rustc_flags,
    get_fixup_env,
    generate_buck_file,
    filter_features_for_availability,
)

__all__ = [
    "PlatformDeps",
    "PlatformNamedDeps",
    "PlatformRustcFlags",
    "CrateFixup",
    "find_matching_version",
    "resolve_dep",
    "extract_deps_from_section",
    "get_dependencies",
    "get_fixup_rustc_flags",
    "get_fixup_env",
    "generate_buck_file",
    "filter_features_for_availability",
]
