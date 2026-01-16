"""Cargo.toml parsing utilities."""

import tomllib
from pathlib import Path


def parse_cargo_toml(crate_dir: Path) -> dict:
    """Parse Cargo.toml and extract relevant information."""
    cargo_toml = crate_dir / "Cargo.toml"
    if not cargo_toml.exists():
        return {}

    with open(cargo_toml, "rb") as f:
        return tomllib.load(f)


def get_crate_name(cargo: dict, fallback: str) -> str:
    """Get the crate name from Cargo.toml."""
    return cargo.get("package", {}).get("name", fallback)


def get_edition(cargo: dict) -> str:
    """Get the Rust edition from Cargo.toml.

    Rust defaults to 2015 when no edition is specified.
    """
    return cargo.get("package", {}).get("edition", "2015")


def get_lib_path(cargo: dict, crate_dir: Path) -> str | None:
    """Determine the library crate root path.

    Always returns an explicit path when possible to avoid Buck2's
    crate_root inference issues with glob(["**/*"]) picking up
    files in examples/, benches/, etc.
    """
    # Check explicit [lib] path
    lib_section = cargo.get("lib", {})
    if "path" in lib_section:
        return lib_section["path"]

    # Check for standard locations - always return explicit path
    src_lib = crate_dir / "src" / "lib.rs"
    if src_lib.exists():
        return "src/lib.rs"

    lib_rs = crate_dir / "lib.rs"
    if lib_rs.exists():
        return "lib.rs"

    return None


def is_proc_macro(cargo: dict) -> bool:
    """Check if the crate is a proc-macro crate."""
    return cargo.get("lib", {}).get("proc-macro", False)


def get_optional_deps(cargo: dict) -> set[str]:
    """Get names of optional dependencies from Cargo.toml."""
    optional = set()
    for dep_name, dep_spec in cargo.get("dependencies", {}).items():
        if isinstance(dep_spec, dict) and dep_spec.get("optional", False):
            # Use the package name if renamed, otherwise use dep_name
            pkg_name = dep_spec.get("package", dep_name)
            optional.add(pkg_name)
            optional.add(dep_name)  # Also add the alias
    return optional


def normalize_crate_name(name: str) -> str:
    """Normalize crate name (Cargo treats hyphens and underscores as equivalent)."""
    return name.replace("-", "_")


def dep_is_available(dep_name: str, available_crates: set[str]) -> bool:
    """Check if a dependency is available (checking hyphen/underscore variants).

    Handles both exact matches (e.g., "quote") and versioned names (e.g., "quote@1.0.43").
    """
    # Check for exact match first
    if dep_name in available_crates:
        return True

    # Check hyphen/underscore variants
    underscore_variant = dep_name.replace("-", "_")
    hyphen_variant = dep_name.replace("_", "-")

    if underscore_variant in available_crates or hyphen_variant in available_crates:
        return True

    # Check for versioned names (e.g., "quote@1.0.43")
    for crate in available_crates:
        if "@" in crate:
            crate_name = crate.split("@")[0]
            if (
                crate_name == dep_name
                or crate_name == underscore_variant
                or crate_name == hyphen_variant
            ):
                return True

    return False


def feature_enables_unavailable_dep(
    feature_name: str, features: dict, available_crates: set[str]
) -> bool:
    """Check if a feature enables an optional dependency that isn't available.

    This handles cases like: default-hasher = ["dep:foldhash"]
    where the feature name differs from the dependency it enables.
    """
    if feature_name not in features:
        return False

    for item in features[feature_name]:
        if item.startswith("dep:"):
            dep_name = item[4:]  # Remove "dep:" prefix
            if not dep_is_available(dep_name, available_crates):
                return True
    return False


def get_default_features(cargo: dict, available_crates: set[str]) -> list[str]:
    """Get the default features from Cargo.toml.

    Features are used for conditional compilation via --cfg feature="...".
    Note:
    - Feature forwarding syntax (e.g., "dep/feature") is filtered out
    - Features that enable optional deps not in available_crates are filtered out
    - Features that enable unavailable deps via dep: syntax are filtered out
    """
    features = cargo.get("features", {})
    default = features.get("default", [])
    optional_deps = get_optional_deps(cargo)

    # Expand feature dependencies (features can enable other features)
    enabled = set(default)
    changed = True
    while changed:
        changed = False
        for feature in list(enabled):
            if feature in features:
                for sub in features[feature]:
                    if sub not in enabled and not sub.startswith("dep:"):
                        enabled.add(sub)
                        changed = True

    # Filter out:
    # 1. Dependency feature forwarding (e.g., "serde_core/std")
    # 2. Features that match optional dep names when that dep isn't available
    # 3. Features that enable unavailable deps via dep: syntax
    result = []
    for f in enabled:
        if "/" in f:
            continue  # Skip feature forwarding
        if f in optional_deps:
            # This feature name matches an optional dep - only enable if dep is available
            if not dep_is_available(f, available_crates):
                continue
        if feature_enables_unavailable_dep(f, features, available_crates):
            continue  # Skip features that enable unavailable deps
        result.append(f)
    return result


def get_cargo_env(cargo: dict, crate_name: str) -> dict[str, str]:
    """Get Cargo environment variables that should be set during build.

    Cargo sets several environment variables that crates can access via env!().
    We replicate the most commonly used ones.
    """
    pkg = cargo.get("package", {})
    version = pkg.get("version", "0.0.0")

    # Parse version components (e.g., "1.2.3-beta.1" -> major=1, minor=2, patch=3, pre=beta.1)
    version_parts = version.split("-", 1)
    version_core = version_parts[0]
    version_pre = version_parts[1] if len(version_parts) > 1 else ""

    core_parts = version_core.split(".")
    major = core_parts[0] if len(core_parts) > 0 else "0"
    minor = core_parts[1] if len(core_parts) > 1 else "0"
    patch = core_parts[2] if len(core_parts) > 2 else "0"

    env = {
        "CARGO_PKG_NAME": crate_name,
        "CARGO_PKG_VERSION": version,
        "CARGO_PKG_VERSION_MAJOR": major,
        "CARGO_PKG_VERSION_MINOR": minor,
        "CARGO_PKG_VERSION_PATCH": patch,
        "CARGO_PKG_VERSION_PRE": version_pre,
    }
    # Add optional fields if present
    if "description" in pkg:
        env["CARGO_PKG_DESCRIPTION"] = pkg["description"]
    if "homepage" in pkg:
        env["CARGO_PKG_HOMEPAGE"] = pkg["homepage"]
    if "repository" in pkg:
        env["CARGO_PKG_REPOSITORY"] = pkg["repository"]
    if "license" in pkg:
        env["CARGO_PKG_LICENSE"] = pkg["license"]
    if "authors" in pkg:
        env["CARGO_PKG_AUTHORS"] = (
            ":".join(pkg["authors"])
            if isinstance(pkg["authors"], list)
            else pkg["authors"]
        )

    return env


def get_version_req(dep_spec) -> str | None:
    """Extract version requirement from a dependency specification."""
    if isinstance(dep_spec, str):
        return dep_spec
    elif isinstance(dep_spec, dict):
        return dep_spec.get("version")
    return None


def extract_dep_features(dep_spec) -> list[str]:
    """Extract features requested for a dependency."""
    if isinstance(dep_spec, str):
        return []  # Simple version string, no features
    elif isinstance(dep_spec, dict):
        features = list(dep_spec.get("features", []))
        # If default-features is not explicitly false, include "default"
        if dep_spec.get("default-features", True) and dep_spec.get(
            "default_features", True
        ):
            features.append("default")
        return features
    return []


def get_dep_package_name(dep_name: str, dep_spec) -> str:
    """Get the actual package name for a dependency (handles renames)."""
    if isinstance(dep_spec, dict) and "package" in dep_spec:
        return dep_spec["package"]
    return dep_name
