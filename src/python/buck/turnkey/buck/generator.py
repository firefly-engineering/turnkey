"""rules.star file generation for Rust crates."""

import sys
from dataclasses import dataclass, field

from turnkey.cargo.toml import (
    normalize_crate_name,
    dep_is_available,
    get_version_req,
    get_optional_deps,
    feature_enables_unavailable_dep,
    is_optional,
)
from turnkey.cargo.features import activate
from turnkey.cargo.semver import best_match
from turnkey.buildsystem.native_library import NativeLibrarySpec
from turnkey.buildsystem.buck2 import buck2_generator
from turnkey.cfg import Platforms, classify_target_platforms


@dataclass
class PlatformDeps:
    """Dependencies categorized by platform.

    common: deps needed on every platform
    by_platform: mapping from select() key to the deps it adds; every
        platform matches one key, or there are none
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


@dataclass
class PlatformRustcFlags:
    """Rustc flags categorized by platform.

    common: flags applied on all platforms
    by_platform: mapping from select() key to the flags it adds; every
        platform matches one key, or there are none
    """

    common: list[str] = field(default_factory=list)
    by_platform: dict[str, list[str]] = field(default_factory=dict)

    def is_empty(self) -> bool:
        return not self.common and not self.by_platform


def find_matching_version(
    pkg_name: str, version_req: str | None, available_crates: set[str]
) -> str | None:
    """Find the vendored "name@version" a dependency on `pkg_name` resolves to.

    Picks the highest vendored version satisfying `version_req` (any version
    when it is None), or None when no vendored version does.
    """
    versions: dict[str, str] = {}
    for crate in available_crates:
        name, sep, version = crate.rpartition("@")
        if sep and normalize_crate_name(name) == normalize_crate_name(pkg_name):
            versions[version] = crate
    match = best_match(version_req, versions)
    return versions[match] if match else None


def resolve_dep(
    pkg_name: str, available_crates: set[str], version_req: str | None = None
) -> str | None:
    """Resolve a package name to a Buck target if it exists in available crates.

    When versioned crates ("name@version") are available for the package, the
    dependency resolves to the highest one satisfying version_req, and to
    nothing if none does: the unversioned "name" symlink points at one of
    them and must not stand in for a version the dependency did not ask for.
    """
    versioned = find_matching_version(pkg_name, version_req, available_crates)
    if versioned:
        # Use the versioned crate name but the unversioned target name
        # e.g., getrandom@0.2.17 has target name "getrandom"
        return f"rustdeps//vendor/{versioned}:" + versioned.split("@")[0]
    if find_matching_version(pkg_name, None, available_crates):
        return None

    # Only unversioned names are available: resolve by name
    for name in (pkg_name, pkg_name.replace("-", "_"), pkg_name.replace("_", "-")):
        if name in available_crates:
            return f"rustdeps//vendor/{name}:{name}"
    return None


def extract_deps_from_section(
    section_deps: dict,
    available_crates: set[str],
    active_optional: set[str],
) -> tuple[list[str], dict[str, str]]:
    """Extract dependencies from a Cargo.toml dependency section.

    Optional dependencies are included only when named in active_optional
    (by their key in the section).

    Returns:
        - List of regular dependency targets
        - Dict of named deps (local_name -> target) for renamed dependencies
    """
    deps = []
    named_deps = {}

    for dep_name, dep_spec in section_deps.items():
        if is_optional(dep_spec) and dep_name not in active_optional:
            continue

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
        if target is None and find_matching_version(pkg_name, None, available_crates):
            # Vendored, but not in a version the requirement matches: say so
            # here rather than leave a missing crate for rustc to report
            print(
                f"warning: {dep_name} = {pkg_name} {version_req!r} matches no "
                "vendored version; leaving it out",
                file=sys.stderr,
            )
        if target:
            if is_renamed:
                # Use normalized local name (hyphens -> underscores) as the crate alias
                local_name = normalize_crate_name(dep_name)
                named_deps[local_name] = target
            else:
                deps.append(target)

    return deps, named_deps


def get_dependencies(
    cargo: dict, available_crates: set[str], features: list[str], platforms: Platforms
) -> tuple[PlatformDeps, PlatformNamedDeps]:
    """Extract dependencies that exist in our vendored crates.

    Note: We only include regular dependencies, not build-dependencies.
    Build scripts require separate rust_build_script rules in Buck2.

    Optional dependencies are included only when the crate's enabled
    features activate them.

    A target-specific table applies on the platforms its spec holds on:
    deps every platform gets are common, the others are keyed as a
    select() (see turnkey.cfg.Platforms), and a table no platform gets is
    dropped.

    Returns:
        - PlatformDeps with common and per-key dependency targets
        - PlatformNamedDeps with common and per-key renamed dependencies
    """
    active_optional = activate(cargo, features).optional_deps

    # Each table's deps, and the platforms it applies on
    sections = [(set(platforms), cargo.get("dependencies", {}))]
    for target_spec, target_config in cargo.get("target", {}).items():
        sections.append(
            (
                classify_target_platforms(target_spec, list(platforms)),
                target_config.get("dependencies", {}),
            )
        )
    extracted = [
        (applies, *extract_deps_from_section(section, available_crates, active_optional))
        for applies, section in sections
    ]

    def deps_on(platform):
        return sorted({d for applies, deps, _ in extracted if platform in applies for d in deps})

    def named_on(platform):
        named = {}
        for applies, _, section_named in extracted:
            if platform in applies:
                named.update(section_named)
        return sorted(named.items())

    common, by_platform = platforms.split(deps_on)
    named_common, named_by_platform = platforms.split(named_on)
    return (
        PlatformDeps(common=common, by_platform=by_platform),
        PlatformNamedDeps(
            common=dict(named_common),
            by_platform={key: dict(items) for key, items in named_by_platform.items()},
        ),
    )


def get_build_script_cfg_flags(
    crate_name: str, version: str, registry: dict, platforms: Platforms
) -> PlatformRustcFlags:
    """Get rustc cfg flags that would be set by a crate's build script.

    Looks up flags from the registry, which supports:
    - Version-specific keys: "crate@version" (takes precedence)
    - Catch-all keys: "crate" (fallback)

    Values can be either:
    - A list of flags (applied on all platforms)
    - A dict whose keys are OS names (Buck2's, e.g. "linux", "macos"),
      mapping to the flags for that OS; any other key's flags apply on
      every platform

    Args:
        crate_name: The crate name (e.g., "serde_json")
        version: The crate version (e.g., "1.0.0")
        registry: Dict mapping crate names/keys to lists or dicts of rustc flags
        platforms: The platforms the cell is built for

    Returns:
        PlatformRustcFlags with common and per-key flags
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
        oses = {p.os for p in platforms}
        common = [flag for key, flags in entry.items() if key not in oses for flag in flags]
        # Flags come in pairs (--cfg foo), so each OS's list is kept whole
        # and in order
        per_os = platforms.branches(lambda p: list(entry.get(p.os, [])))
        if per_os is None:
            return PlatformRustcFlags(common=common + list(entry.get(next(iter(platforms)).os, [])))
        return PlatformRustcFlags(common=common, by_platform=per_os)

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
    # Every platform matches a key, even with nothing to add: there's no
    # default branch, so an unlisted platform fails instead of missing values
    for platform_key in sorted(by_platform.keys()):
        items = by_platform[platform_key]
        if not items:
            lines.append(f'{indent}    "{platform_key}": [],')
            continue
        lines.append(f'{indent}    "{platform_key}": [')
        ordered = sorted(set(items)) if dedup_sort else items
        for item in ordered:
            lines.append(f"{indent}        {format_item(item)},")
        lines.append(f"{indent}    ],")
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
            lines.append(f'{indent}    "{platform_key}": {{}},')
            continue
        lines.append(f'{indent}    "{platform_key}": {{')
        for local_name, target in sorted(items.items()):
            lines.append(f'{indent}        "{local_name}": "{target}",')
        lines.append(f"{indent}    }},")
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
