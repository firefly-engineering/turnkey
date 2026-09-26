"""Cargo.toml parsing and feature unification library."""

from .toml import (
    parse_cargo_toml,
    get_crate_name,
    find_workspace_root,
    get_edition,
    get_lib_path,
    is_proc_macro,
    get_optional_deps,
    normalize_crate_name,
    dep_is_available,
    feature_enables_unavailable_dep,
    get_default_features,
    get_cargo_env,
    get_version_req,
    extract_dep_features,
    get_dep_package_name,
)
from .features import (
    Activation,
    activate,
    parse_feature_forwarding,
    compute_unified_features,
    load_overrides,
    load_requested,
)
from .semver import best_match, matches

__all__ = [
    # toml.py
    "parse_cargo_toml",
    "get_crate_name",
    "find_workspace_root",
    "get_edition",
    "get_lib_path",
    "is_proc_macro",
    "get_optional_deps",
    "normalize_crate_name",
    "dep_is_available",
    "feature_enables_unavailable_dep",
    "get_default_features",
    "get_cargo_env",
    "get_version_req",
    "extract_dep_features",
    "get_dep_package_name",
    # features.py
    "Activation",
    "activate",
    "parse_feature_forwarding",
    "compute_unified_features",
    "load_overrides",
    "load_requested",
    # semver.py
    "best_match",
    "matches",
]
