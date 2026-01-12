#!/usr/bin/env python3
"""
Compute unified features for all Rust crates in a vendor directory.

This script implements Cargo-style feature unification:
1. Parse all Cargo.toml files to find dependency feature requirements
2. Compute the union of all features requested by any dependent
3. Output JSON mapping crate names to their unified feature sets

This matches Cargo's behavior where if any crate requires feature X on crate Y,
crate Y is built with feature X enabled.
"""

import json
import sys
import tomllib
from collections import defaultdict
from pathlib import Path


def normalize_crate_name(name: str) -> str:
    """Normalize crate name (Cargo treats hyphens and underscores as equivalent)."""
    return name.replace("-", "_")


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


def extract_dep_features(dep_spec) -> list[str]:
    """Extract features requested for a dependency."""
    if isinstance(dep_spec, str):
        return []  # Simple version string, no features
    elif isinstance(dep_spec, dict):
        features = list(dep_spec.get("features", []))
        # If default-features is not explicitly false, include "default"
        if dep_spec.get("default-features", True) and dep_spec.get("default_features", True):
            features.append("default")
        return features
    return []


def get_dep_package_name(dep_name: str, dep_spec) -> str:
    """Get the actual package name for a dependency (handles renames)."""
    if isinstance(dep_spec, dict) and "package" in dep_spec:
        return dep_spec["package"]
    return dep_name


def parse_feature_forwarding(feature_item: str) -> tuple[str, str] | None:
    """
    Parse feature forwarding syntax: "dep/feature" or "dep?/feature"

    Returns (dep_name, feature_name) or None if not feature forwarding.
    """
    if "/" not in feature_item:
        return None

    # Handle optional dep syntax: "dep?/feature"
    if "?" in feature_item:
        dep_part, feature = feature_item.split("/", 1)
        dep_name = dep_part.rstrip("?")
    else:
        dep_name, feature = feature_item.split("/", 1)

    return (dep_name, feature)


def collect_feature_requirements(vendor_dir: Path) -> tuple[dict[str, set[str]], dict[str, dict]]:
    """
    Scan all crates and collect feature requirements from their dependents.

    Returns:
        - Dict mapping normalized crate names to sets of required features
        - Dict mapping crate names to their Cargo.toml data (for feature expansion)
    """
    # Map: normalized_crate_name -> set of features required by dependents
    required_features: dict[str, set[str]] = defaultdict(set)
    # Map: crate_name -> Cargo.toml data
    crate_cargo_data: dict[str, dict] = {}

    # Find all crate directories (both versioned and unversioned)
    crate_dirs = []
    for item in vendor_dir.iterdir():
        if item.is_dir() and not item.is_symlink():
            crate_dirs.append(item)

    # First pass: Parse all Cargo.toml files and collect direct feature requirements
    for crate_dir in crate_dirs:
        cargo = parse_cargo_toml(crate_dir)
        if not cargo:
            continue

        # Store cargo data for later feature expansion
        dir_name = crate_dir.name
        if "@" in dir_name:
            fallback_name = dir_name.split("@")[0]
        else:
            fallback_name = dir_name
        crate_name = get_crate_name(cargo, fallback_name)
        crate_cargo_data[crate_name] = cargo

        # Process regular dependencies
        for dep_name, dep_spec in cargo.get("dependencies", {}).items():
            pkg_name = get_dep_package_name(dep_name, dep_spec)
            features = extract_dep_features(dep_spec)
            normalized = normalize_crate_name(pkg_name)
            required_features[normalized].update(features)

        # Process dev-dependencies (they can affect feature requirements too)
        for dep_name, dep_spec in cargo.get("dev-dependencies", {}).items():
            pkg_name = get_dep_package_name(dep_name, dep_spec)
            features = extract_dep_features(dep_spec)
            normalized = normalize_crate_name(pkg_name)
            required_features[normalized].update(features)

        # Process target-specific dependencies
        for target_spec, target_config in cargo.get("target", {}).items():
            for dep_name, dep_spec in target_config.get("dependencies", {}).items():
                pkg_name = get_dep_package_name(dep_name, dep_spec)
                features = extract_dep_features(dep_spec)
                normalized = normalize_crate_name(pkg_name)
                required_features[normalized].update(features)

    return required_features, crate_cargo_data


def collect_forwarded_features(
    crate_cargo_data: dict[str, dict],
    required_features: dict[str, set[str]],
) -> dict[str, set[str]]:
    """
    Second pass: Collect features that are forwarded to dependencies.

    When a crate has: alloc = ["zerovec/alloc"]
    And alloc is enabled, zerovec should get the "alloc" feature.

    This iterates until no new features are discovered (fixed point).
    """
    forwarded: dict[str, set[str]] = defaultdict(set)

    # Iterate until no changes (feature forwarding can be transitive)
    changed = True
    iterations = 0
    max_iterations = 100  # Safety limit

    while changed and iterations < max_iterations:
        changed = False
        iterations += 1

        for crate_name, cargo in crate_cargo_data.items():
            normalized_crate = normalize_crate_name(crate_name)
            crate_features_def = cargo.get("features", {})

            # Get all features that will be enabled for this crate
            default_features = set(crate_features_def.get("default", []))
            requested = required_features.get(normalized_crate, set())
            all_enabled = default_features | requested | forwarded.get(normalized_crate, set())

            # Expand features to find forwarding
            to_process = list(all_enabled)
            processed = set()

            while to_process:
                feature = to_process.pop()
                if feature in processed:
                    continue
                processed.add(feature)

                # Handle "default" specially
                if feature == "default" and "default" in crate_features_def:
                    to_process.extend(crate_features_def["default"])
                    continue

                # Check what this feature enables
                if feature in crate_features_def:
                    for sub in crate_features_def[feature]:
                        # Check for feature forwarding
                        fwd = parse_feature_forwarding(sub)
                        if fwd:
                            dep_name, dep_feature = fwd
                            normalized_dep = normalize_crate_name(dep_name)
                            if dep_feature not in forwarded[normalized_dep]:
                                forwarded[normalized_dep].add(dep_feature)
                                changed = True
                        elif not sub.startswith("dep:") and sub not in processed:
                            to_process.append(sub)

    return forwarded


def expand_features(
    crate_name: str,
    requested: set[str],
    crate_features: dict[str, list[str]],
) -> set[str]:
    """
    Expand feature set by following feature dependencies.

    In Cargo, features can enable other features:
        [features]
        full = ["parsing", "printing"]

    This expands "full" to include "parsing" and "printing".
    """
    expanded = set()
    to_process = list(requested)

    while to_process:
        feature = to_process.pop()
        if feature in expanded:
            continue

        # Handle "default" specially
        if feature == "default":
            if "default" in crate_features:
                to_process.extend(crate_features["default"])
            continue

        expanded.add(feature)

        # If this feature enables other features, add them
        if feature in crate_features:
            for sub_feature in crate_features[feature]:
                # Skip dep: syntax and feature forwarding (dep/feature)
                if sub_feature.startswith("dep:") or "/" in sub_feature:
                    continue
                if sub_feature not in expanded:
                    to_process.append(sub_feature)

    return expanded


def compute_unified_features(vendor_dir: Path, overrides: dict) -> dict[str, list[str]]:
    """
    Compute unified features for all crates.

    Args:
        vendor_dir: Path to vendor directory containing crate sources
        overrides: Manual feature overrides from rust-features.toml

    Returns:
        Dict mapping crate names to sorted lists of features
    """
    # Collect what features are requested by dependents
    required_features, crate_cargo_data = collect_feature_requirements(vendor_dir)

    # Collect features forwarded through feature definitions (e.g., "alloc" = ["zerovec/alloc"])
    forwarded_features = collect_forwarded_features(crate_cargo_data, required_features)

    # Merge forwarded features into required features
    for crate_name, features in forwarded_features.items():
        required_features[crate_name].update(features)

    # Build a map of crate features definitions for expansion
    crate_feature_defs: dict[str, dict[str, list[str]]] = {}
    crate_dirs = [d for d in vendor_dir.iterdir() if d.is_dir() and not d.is_symlink()]

    for crate_dir in crate_dirs:
        cargo = parse_cargo_toml(crate_dir)
        if not cargo:
            continue

        dir_name = crate_dir.name
        if "@" in dir_name:
            fallback_name = dir_name.split("@")[0]
        else:
            fallback_name = dir_name

        crate_name = get_crate_name(cargo, fallback_name)
        normalized = normalize_crate_name(crate_name)
        crate_feature_defs[normalized] = cargo.get("features", {})

    # Compute final unified features for each crate
    unified: dict[str, list[str]] = {}

    for crate_dir in crate_dirs:
        cargo = parse_cargo_toml(crate_dir)
        if not cargo:
            continue

        dir_name = crate_dir.name
        if "@" in dir_name:
            fallback_name = dir_name.split("@")[0]
        else:
            fallback_name = dir_name

        crate_name = get_crate_name(cargo, fallback_name)
        normalized = normalize_crate_name(crate_name)

        # Check for manual override first
        if crate_name in overrides:
            override = overrides[crate_name]
            if isinstance(override, list):
                # Complete replacement
                unified[crate_name] = sorted(override)
                continue
            elif isinstance(override, dict):
                # Additive/subtractive - apply after computing base
                pass

        # Start with default features
        default_features = set(cargo.get("features", {}).get("default", []))

        # Add features required by dependents
        requested = required_features.get(normalized, set())
        all_requested = default_features | requested

        # Expand feature dependencies
        feature_defs = crate_feature_defs.get(normalized, {})
        expanded = expand_features(crate_name, all_requested, feature_defs)

        # Apply additive/subtractive overrides if present
        if crate_name in overrides and isinstance(overrides[crate_name], dict):
            override = overrides[crate_name]
            if "add" in override:
                expanded.update(override["add"])
            if "remove" in override:
                expanded -= set(override["remove"])

        # Filter out feature forwarding syntax (not valid rustc flags)
        expanded = {f for f in expanded if "/" not in f and not f.startswith("dep:")}

        unified[crate_name] = sorted(expanded)

    return unified


def load_overrides(overrides_file: Path | None) -> dict:
    """Load feature overrides from rust-features.toml."""
    if overrides_file is None or not overrides_file.exists():
        return {}

    with open(overrides_file, "rb") as f:
        data = tomllib.load(f)

    return data.get("overrides", {})


def main():
    if len(sys.argv) < 2:
        print("Usage: compute-unified-features.py <vendor_dir> [overrides_file]", file=sys.stderr)
        sys.exit(1)

    vendor_dir = Path(sys.argv[1])
    overrides_file = Path(sys.argv[2]) if len(sys.argv) > 2 else None

    overrides = load_overrides(overrides_file)
    unified = compute_unified_features(vendor_dir, overrides)

    # Output as JSON
    print(json.dumps(unified, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
