"""rules.star file generation for Rust crates."""

from python.cargo.toml import (
    normalize_crate_name,
    dep_is_available,
    get_version_req,
    get_optional_deps,
    feature_enables_unavailable_dep,
)

try:
    from cfg import is_linux_compatible_target
except ImportError:
    from python.cfg import is_linux_compatible_target


def find_matching_version(
    pkg_name: str, version_req: str | None, available_crates: set[str]
) -> str | None:
    """Find a versioned crate that matches the version requirement.

    When multiple versions exist, we need to select the right one based on semver.
    """
    if not version_req:
        return None

    # Parse version requirement (simple parsing for common cases)
    # Handle formats like "0.2.10", "^0.2", ">=1.0", "=1.2.3"
    req = version_req.lstrip("^~>=<= ")
    req_parts = req.split(".")
    if not req_parts:
        return None

    major = req_parts[0]
    minor = req_parts[1] if len(req_parts) > 1 else None

    # Look for versioned crates matching this requirement
    # Try exact match first, then compatible versions
    candidates = []
    for crate in available_crates:
        if "@" not in crate:
            continue
        name, version = crate.rsplit("@", 1)
        if (
            name != pkg_name
            and name != pkg_name.replace("-", "_")
            and name != pkg_name.replace("_", "-")
        ):
            continue

        v_parts = version.split(".")
        v_major = v_parts[0] if len(v_parts) > 0 else "0"
        v_minor = v_parts[1] if len(v_parts) > 1 else "0"

        # For 0.x versions, minor version must match (0.2 != 0.3)
        # For 1.x+, major version must match
        if major == "0":
            if v_major == major and (minor is None or v_minor == minor):
                candidates.append((crate, version))
        else:
            if v_major == major:
                candidates.append((crate, version))

    if not candidates:
        return None

    # Return the highest matching version
    # Sort by version parts (simple string sort works for most cases)
    candidates.sort(
        key=lambda x: [int(p) for p in x[1].split(".")[:3] if p.isdigit()], reverse=True
    )
    return candidates[0][0]


def resolve_dep(
    pkg_name: str, available_crates: set[str], version_req: str | None = None
) -> str | None:
    """Resolve a package name to a Buck target if it exists in available crates.

    When version_req is provided and multiple versions exist, selects the right version.
    """
    # First, try to find a versioned match if version requirement is specified
    if version_req:
        versioned = find_matching_version(pkg_name, version_req, available_crates)
        if versioned:
            # Use the versioned crate name but the unversioned target name
            # e.g., getrandom@0.2.17 has target name "getrandom"
            return f'rustdeps//vendor/{versioned}:' + versioned.split("@")[0]

    # Fall back to unversioned symlink
    if pkg_name in available_crates:
        return f"rustdeps//vendor/{pkg_name}:{pkg_name}"
    elif pkg_name.replace("-", "_") in available_crates:
        normalized = pkg_name.replace("-", "_")
        return f"rustdeps//vendor/{normalized}:{normalized}"
    elif pkg_name.replace("_", "-") in available_crates:
        normalized = pkg_name.replace("_", "-")
        return f"rustdeps//vendor/{normalized}:{normalized}"
    return None


def extract_deps_from_section(
    section_deps: dict,
    available_crates: set[str],
) -> tuple[list[str], dict[str, str]]:
    """Extract dependencies from a Cargo.toml dependency section.

    Returns:
        - List of regular dependency targets
        - Dict of named deps (local_name -> target) for renamed dependencies
    """
    deps = []
    named_deps = {}

    for dep_name, dep_spec in section_deps.items():
        # Get the actual package name (may be different from dependency key)
        if isinstance(dep_spec, dict) and "package" in dep_spec:
            pkg_name = dep_spec["package"]
            is_renamed = True
        else:
            pkg_name = dep_name
            is_renamed = False

        # Get version requirement for proper version selection
        version_req = get_version_req(dep_spec)
        target = resolve_dep(pkg_name, available_crates, version_req)
        if target:
            if is_renamed:
                # Use normalized local name (hyphens -> underscores) as the crate alias
                local_name = normalize_crate_name(dep_name)
                named_deps[local_name] = target
            else:
                deps.append(target)

    return deps, named_deps


def get_dependencies(
    cargo: dict, available_crates: set[str]
) -> tuple[list[str], dict[str, str]]:
    """Extract dependencies that exist in our vendored crates.

    Note: We only include regular dependencies, not build-dependencies.
    Build scripts require separate rust_build_script rules in Buck2.
    Also, we only include dependencies compatible with Linux.

    Returns:
        - List of regular dependency targets
        - Dict of named deps (local_name -> target) for renamed dependencies
    """
    deps = []
    named_deps = {}

    # Standard dependencies (not build-dependencies)
    section_deps = cargo.get("dependencies", {})
    section_deps_list, section_named = extract_deps_from_section(
        section_deps, available_crates
    )
    deps.extend(section_deps_list)
    named_deps.update(section_named)

    # Target-specific dependencies - only for Linux-compatible targets
    for target_spec, target_config in cargo.get("target", {}).items():
        if is_linux_compatible_target(target_spec):
            section_deps = target_config.get("dependencies", {})
            section_deps_list, section_named = extract_deps_from_section(
                section_deps, available_crates
            )
            deps.extend(section_deps_list)
            named_deps.update(section_named)

    return deps, named_deps


def get_build_script_cfg_flags(
    crate_name: str, version: str, registry: dict
) -> list[str]:
    """Get rustc cfg flags that would be set by a crate's build script.

    Looks up flags from the registry, which supports:
    - Version-specific keys: "crate@version" (takes precedence)
    - Catch-all keys: "crate" (fallback)

    Args:
        crate_name: The crate name (e.g., "serde_json")
        version: The crate version (e.g., "1.0.0")
        registry: Dict mapping crate names/keys to lists of rustc flags

    Returns:
        List of rustc flags for the crate
    """
    # Try versioned key first (e.g., "rustix@0.39.0")
    versioned_key = f"{crate_name}@{version}"
    if versioned_key in registry:
        return registry[versioned_key]

    # Fall back to crate name (catch-all)
    if crate_name in registry:
        return registry[crate_name]

    return []


def get_native_library_info(crate_name: str, version: str) -> dict | None:
    """Get info about native libraries for crates with pre-built native code.

    Some crates (like ring) have native C/assembly code that we pre-compile
    in Nix. This returns info needed to create a prebuilt_cxx_library rule
    and configure the linker.

    Returns dict with:
        - lib_name: The library name (without lib prefix and .a suffix)
        - static_lib_path: Path to the static library file
        - link_search_path: Path for -L flag (relative to crate dir)
    """
    if crate_name == "ring":
        # ring's native crypto library is pre-compiled and placed in out_dir/
        # The library name follows ring's versioning: libring_core_0_17_<patch>.a
        patch = version.split(".")[-1] if version else "0"
        lib_name = f"ring_core_0_17_{patch}__"
        return {
            "lib_name": lib_name,
            "static_lib_path": f"out_dir/lib{lib_name}.a",
            "link_search_path": "out_dir",
        }
    return None


def generate_buck_file(
    crate_name: str,
    edition: str,
    crate_root: str | None,
    deps: list[str],
    named_deps: dict[str, str],
    proc_macro: bool,
    features: list[str],
    env: dict[str, str],
    rustc_flags: list[str],
    native_lib_info: dict | None = None,
) -> str:
    """Generate BUCK file content."""
    # Initialize linker_flags
    linker_flags = []

    # Determine which rules we need to load
    rules_to_load = ["rust_library"]
    if native_lib_info:
        rules_to_load.extend(["prebuilt_cxx_library", "export_file"])

    # Format rules for load statement: "rule1", "rule2"
    rules_str = ", ".join(f'"{r}"' for r in rules_to_load)

    lines = [
        "# Auto-generated by turnkey rust-deps-cell",
        f'load("@prelude//:rules.bzl", {rules_str})',
        "",
    ]

    # Generate prebuilt_cxx_library for native libraries
    if native_lib_info:
        lib_name = native_lib_info["lib_name"]
        static_lib_path = native_lib_info["static_lib_path"]
        link_search_path = native_lib_info.get("link_search_path", "out_dir")
        # Use export_file to expose the library, then reference it in prebuilt_cxx_library
        lines.extend(
            [
                "# Native crypto library pre-compiled in Nix",
                "export_file(",
                f'    name = "{lib_name}_file",',
                f'    src = "{static_lib_path}",',
                '    visibility = ["PUBLIC"],',
                ")",
                "",
                "prebuilt_cxx_library(",
                f'    name = "{lib_name}",',
                f'    static_lib = ":{lib_name}_file",',
                "    link_whole = True,",
                '    preferred_linkage = "static",',
                '    visibility = ["PUBLIC"],',
                ")",
                "",
            ]
        )
        # Add the native library as a dependency
        deps = deps + [f":{lib_name}"]
        # Add -L flag so rustc can find the library during compilation
        rustc_flags = rustc_flags + [f"-Lnative={link_search_path}"]

    lines.extend(
        [
            "rust_library(",
            f'    name = "{crate_name}",',
            '    srcs = glob(["**/*"]),',
            f'    edition = "{edition}",',
        ]
    )

    if proc_macro:
        lines.append("    proc_macro = True,")

    if features:
        lines.append("    features = [")
        for feature in sorted(features):
            lines.append(f'        "{feature}",')
        lines.append("    ],")

    if crate_root:
        lines.append(f'    crate_root = "{crate_root}",')

    if deps:
        lines.append("    deps = [")
        for dep in sorted(set(deps)):
            lines.append(f'        "{dep}",')
        lines.append("    ],")

    # Add named_deps for renamed dependencies
    if named_deps:
        lines.append("    named_deps = {")
        for local_name, target in sorted(named_deps.items()):
            lines.append(f'        "{local_name}": "{target}",')
        lines.append("    },")

    # Add Cargo environment variables
    if env:
        lines.append("    env = {")
        for key, value in sorted(env.items()):
            # Escape special characters and normalize whitespace
            # Replace newlines with spaces for single-line values
            escaped_value = value.replace("\n", " ").replace("\r", " ")
            escaped_value = escaped_value.replace("\\", "\\\\").replace('"', '\\"')
            lines.append(f'        "{key}": "{escaped_value}",')
        lines.append("    },")

    # Add rustc flags (for build script cfg emulation)
    if rustc_flags:
        lines.append("    rustc_flags = [")
        for flag in rustc_flags:
            lines.append(f'        "{flag}",')
        lines.append("    ],")

    # Add exported_linker_flags for native libraries (propagates to dependents)
    if linker_flags:
        lines.append("    exported_linker_flags = [")
        for flag in linker_flags:
            lines.append(f'        "{flag}",')
        lines.append("    ],")

    lines.extend(
        [
            '    visibility = ["PUBLIC"],',
            ")",
            "",
        ]
    )

    return "\n".join(lines)


def filter_features_for_availability(
    features: list[str],
    cargo: dict,
    available_crates: set[str],
) -> list[str]:
    """Filter out features that enable unavailable optional dependencies."""
    cargo_features = cargo.get("features", {})
    optional_deps = get_optional_deps(cargo)

    result = []
    for f in features:
        # Skip feature forwarding (shouldn't be here, but be safe)
        if "/" in f:
            continue
        # Check if feature matches an optional dep that's not available
        if f in optional_deps:
            if not dep_is_available(f, available_crates):
                continue
        # Check if feature enables unavailable deps via dep: syntax
        if feature_enables_unavailable_dep(f, cargo_features, available_crates):
            continue
        result.append(f)
    return result
