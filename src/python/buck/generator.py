"""rules.star file generation for Rust crates."""

from dataclasses import dataclass, field

from python.cargo.toml import (
    normalize_crate_name,
    dep_is_available,
    get_version_req,
    get_optional_deps,
    feature_enables_unavailable_dep,
)
from python.buildsystem.native_library import NativeLibrarySpec
from python.buildsystem.buck2 import buck2_generator

try:
    from cfg import classify_target_platforms, SUPPORTED_PLATFORMS
except ImportError:
    from python.cfg import classify_target_platforms, SUPPORTED_PLATFORMS


ALL_PLATFORM_KEYS = set(SUPPORTED_PLATFORMS.keys())


@dataclass
class PlatformDeps:
    """Dependencies categorized by platform.

    common: deps needed on all supported platforms
    by_platform: mapping from config_setting key to platform-specific deps
    """

    common: list[str] = field(default_factory=list)
    by_platform: dict[str, list[str]] = field(default_factory=dict)

    def is_empty(self) -> bool:
        return not self.common and not self.by_platform


@dataclass
class PlatformNamedDeps:
    """Named (renamed) dependencies categorized by platform."""

    common: dict[str, str] = field(default_factory=dict)
    by_platform: dict[str, dict[str, str]] = field(default_factory=dict)

    def is_empty(self) -> bool:
        return not self.common and not self.by_platform


# Mapping from short platform names (used in Nix fixups) to Buck2 config_setting keys.
_PLATFORM_SHORT_NAMES = {
    "linux": "config//os:linux",
    "macos": "config//os:macos",
}


@dataclass
class PlatformRustcFlags:
    """Rustc flags categorized by platform.

    common: flags applied on all platforms
    by_platform: mapping from config_setting key to platform-specific flags
    """

    common: list[str] = field(default_factory=list)
    by_platform: dict[str, list[str]] = field(default_factory=dict)

    def is_empty(self) -> bool:
        return not self.common and not self.by_platform


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
) -> tuple[PlatformDeps, PlatformNamedDeps]:
    """Extract dependencies that exist in our vendored crates.

    Note: We only include regular dependencies, not build-dependencies.
    Build scripts require separate rust_build_script rules in Buck2.

    Target-specific dependencies are classified by platform and emitted
    using Buck2 select() so the right deps are used on each OS.

    Returns:
        - PlatformDeps with common and per-platform dependency targets
        - PlatformNamedDeps with common and per-platform renamed dependencies
    """
    platform_deps = PlatformDeps()
    platform_named = PlatformNamedDeps()

    # Standard dependencies (not build-dependencies) - always common
    section_deps = cargo.get("dependencies", {})
    common_deps, common_named = extract_deps_from_section(
        section_deps, available_crates
    )
    platform_deps.common.extend(common_deps)
    platform_named.common.update(common_named)

    # Target-specific dependencies - classify by platform
    for target_spec, target_config in cargo.get("target", {}).items():
        matching_platforms = classify_target_platforms(target_spec)
        if not matching_platforms:
            continue  # Not compatible with any supported platform

        section_deps = target_config.get("dependencies", {})
        section_deps_list, section_named = extract_deps_from_section(
            section_deps, available_crates
        )
        if not section_deps_list and not section_named:
            continue

        if matching_platforms == ALL_PLATFORM_KEYS:
            # Matches all platforms - treat as common
            platform_deps.common.extend(section_deps_list)
            platform_named.common.update(section_named)
        else:
            # Platform-specific
            for platform_key in matching_platforms:
                if section_deps_list:
                    platform_deps.by_platform.setdefault(platform_key, []).extend(
                        section_deps_list
                    )
                if section_named:
                    platform_named.by_platform.setdefault(platform_key, {}).update(
                        section_named
                    )

    return platform_deps, platform_named


def get_build_script_cfg_flags(
    crate_name: str, version: str, registry: dict
) -> PlatformRustcFlags:
    """Get rustc cfg flags that would be set by a crate's build script.

    Looks up flags from the registry, which supports:
    - Version-specific keys: "crate@version" (takes precedence)
    - Catch-all keys: "crate" (fallback)

    Values can be either:
    - A list of flags (applied on all platforms)
    - A dict with platform keys ("linux", "macos") mapping to flag lists

    Args:
        crate_name: The crate name (e.g., "serde_json")
        version: The crate version (e.g., "1.0.0")
        registry: Dict mapping crate names/keys to lists or dicts of rustc flags

    Returns:
        PlatformRustcFlags with common and per-platform flags
    """
    # Try versioned key first (e.g., "rustix@0.39.0")
    versioned_key = f"{crate_name}@{version}"
    entry = registry.get(versioned_key) or registry.get(crate_name)

    if entry is None:
        return PlatformRustcFlags()

    if isinstance(entry, list):
        # Simple list: common flags for all platforms
        return PlatformRustcFlags(common=entry)

    if isinstance(entry, dict):
        # Dict with platform keys: platform-specific flags
        result = PlatformRustcFlags()
        for short_name, config_key in _PLATFORM_SHORT_NAMES.items():
            if short_name in entry:
                result.by_platform[config_key] = entry[short_name]
        # Any keys not in _PLATFORM_SHORT_NAMES are treated as common
        for key, flags in entry.items():
            if key not in _PLATFORM_SHORT_NAMES:
                result.common.extend(flags)
        return result

    return PlatformRustcFlags()


def _format_select(
    by_platform: dict[str, list[str]],
    indent: str,
    format_item,
    dedup_sort: bool = True,
) -> str:
    """Format a select() expression for platform-specific values.

    Args:
        by_platform: mapping from config_setting key to list of items
        indent: base indentation string
        format_item: function to format each item as a string
        dedup_sort: if True, deduplicate and sort items (good for deps);
                    if False, preserve order (needed for rustc flags)
    """
    lines = []
    lines.append(f"{indent}select({{")
    for platform_key in sorted(by_platform.keys()):
        items = by_platform[platform_key]
        if not items:
            continue
        lines.append(f'{indent}    "{platform_key}": [')
        ordered = sorted(set(items)) if dedup_sort else items
        for item in ordered:
            lines.append(f"{indent}        {format_item(item)},")
        lines.append(f"{indent}    ],")
    lines.append(f'{indent}    "DEFAULT": [],')
    lines.append(f"{indent}}})")
    return "\n".join(lines)


def _format_named_select(
    by_platform: dict[str, dict[str, str]],
    indent: str,
) -> str:
    """Format a select() expression for platform-specific named deps."""
    lines = []
    lines.append(f"{indent}select({{")
    for platform_key in sorted(by_platform.keys()):
        items = by_platform[platform_key]
        if not items:
            continue
        lines.append(f'{indent}    "{platform_key}": {{')
        for local_name, target in sorted(items.items()):
            lines.append(f'{indent}        "{local_name}": "{target}",')
        lines.append(f"{indent}    }},")
    lines.append(f'{indent}    "DEFAULT": {{}},')
    lines.append(f"{indent}}})")
    return "\n".join(lines)


def generate_buck_file(
    crate_name: str,
    edition: str,
    crate_root: str | None,
    platform_deps: PlatformDeps,
    platform_named_deps: PlatformNamedDeps,
    proc_macro: bool,
    features: list[str],
    env: dict[str, str],
    rustc_flags: PlatformRustcFlags,
    native_lib_info: dict | None = None,
) -> str:
    """Generate BUCK file content."""
    # Initialize linker_flags
    linker_flags = []

    # Determine which rules we need to load
    rules_to_load = ["rust_library"]

    # Native library rules prefix content
    native_lib_content = ""

    # Collect all deps from common to pass to native library
    deps = list(platform_deps.common)

    # Generate native library rules using the abstraction
    if native_lib_info:
        spec = NativeLibrarySpec.from_dict(native_lib_info)
        generated = buck2_generator.generate(spec)

        rules_to_load.extend(generated.rules_to_load)
        native_lib_content = generated.rules_content
        deps = deps + generated.extra_deps
        rustc_flags = PlatformRustcFlags(
            common=rustc_flags.common + generated.extra_rustc_flags,
            by_platform=rustc_flags.by_platform,
        )

    # Format rules for load statement: "rule1", "rule2"
    rules_str = ", ".join(f'"{r}"' for r in rules_to_load)

    lines = [
        "# Auto-generated by turnkey rust-deps-cell",
        f'load("@prelude//:rules.bzl", {rules_str})',
        "",
    ]

    # Add native library rules if present
    if native_lib_content:
        lines.append(native_lib_content)

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

    # Deps: common deps + optional select() for platform-specific
    has_common_deps = bool(deps)
    has_platform_deps = bool(platform_deps.by_platform)

    if has_common_deps and has_platform_deps:
        lines.append("    deps = [")
        for dep in sorted(set(deps)):
            lines.append(f'        "{dep}",')
        lines.append("    ] +")
        select_str = _format_select(
            platform_deps.by_platform, "    ", lambda d: f'"{d}"'
        )
        lines.append(select_str + ",")
    elif has_common_deps:
        lines.append("    deps = [")
        for dep in sorted(set(deps)):
            lines.append(f'        "{dep}",')
        lines.append("    ],")
    elif has_platform_deps:
        lines.append("    deps =")
        select_str = _format_select(
            platform_deps.by_platform, "    ", lambda d: f'"{d}"'
        )
        lines.append(select_str + ",")

    # Named deps: common + optional select() for platform-specific
    named_deps = platform_named_deps.common
    has_common_named = bool(named_deps)
    has_platform_named = bool(platform_named_deps.by_platform)

    if has_common_named and has_platform_named:
        lines.append("    named_deps = {")
        for local_name, target in sorted(named_deps.items()):
            lines.append(f'        "{local_name}": "{target}",')
        lines.append("    } |")
        named_select_str = _format_named_select(
            platform_named_deps.by_platform, "    "
        )
        lines.append(named_select_str + ",")
    elif has_common_named:
        lines.append("    named_deps = {")
        for local_name, target in sorted(named_deps.items()):
            lines.append(f'        "{local_name}": "{target}",')
        lines.append("    },")
    elif has_platform_named:
        lines.append("    named_deps =")
        named_select_str = _format_named_select(
            platform_named_deps.by_platform, "    "
        )
        lines.append(named_select_str + ",")

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
    has_common_flags = bool(rustc_flags.common)
    has_platform_flags = bool(rustc_flags.by_platform)

    def _escape_flag(flag):
        return flag.replace("\\", "\\\\").replace('"', '\\"')

    if has_common_flags and has_platform_flags:
        lines.append("    rustc_flags = [")
        for flag in rustc_flags.common:
            lines.append(f'        "{_escape_flag(flag)}",')
        lines.append("    ] +")
        select_str = _format_select(
            rustc_flags.by_platform, "    ", lambda f: f'"{_escape_flag(f)}"',
            dedup_sort=False,
        )
        lines.append(select_str + ",")
    elif has_common_flags:
        lines.append("    rustc_flags = [")
        for flag in rustc_flags.common:
            lines.append(f'        "{_escape_flag(flag)}",')
        lines.append("    ],")
    elif has_platform_flags:
        lines.append("    rustc_flags =")
        select_str = _format_select(
            rustc_flags.by_platform, "    ", lambda f: f'"{_escape_flag(f)}"',
            dedup_sort=False,
        )
        lines.append(select_str + ",")

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
