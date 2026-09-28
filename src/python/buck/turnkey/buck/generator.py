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


@dataclass
class CrateFixup:
    """A crate's fixup, as turnkey resolved it for one locked version.

    Written by nix/lib/fixups/resolve.nix, one per "name@version":
    rustc flags and env are layered as the flags every platform gets
    (common) and those a platform's OS, CPU, or OS and CPU pair
    ("<os>-<cpu>") adds; native libraries exist only for the platform the
    cell was built on.
    """

    out_dir: bool = False
    rustc_flags: dict = field(default_factory=lambda: {"common": [], "os": {}, "cpu": {}, "platform": {}})
    env: dict = field(default_factory=lambda: {"common": {}, "os": {}, "cpu": {}, "platform": {}})
    native_libraries: list[dict] = field(default_factory=list)

    @classmethod
    def from_dict(cls, d: dict) -> "CrateFixup":
        return cls(
            out_dir=d.get("outDir", False),
            rustc_flags=d.get("rustcFlags", {"common": [], "os": {}, "cpu": {}, "platform": {}}),
            env=d.get("env", {"common": {}, "os": {}, "cpu": {}, "platform": {}}),
            native_libraries=d.get("nativeLibraries", []),
        )


def _overlays_on(layers: dict, p) -> list:
    """What a fixup's overlays give platform p, most general first: its OS's, its CPU's, then its OS and CPU pair's."""
    return [
        layers.get("os", {}).get(p.os),
        layers.get("cpu", {}).get(p.cpu),
        layers.get("platform", {}).get(f"{p.os}-{p.cpu}"),
    ]


def get_fixup_rustc_flags(fixup: CrateFixup, platforms: Platforms) -> PlatformRustcFlags:
    """The rustc flags a crate's fixup gives it, as common flags and select() branches.

    Flags come in pairs (--cfg foo), so each platform's list is kept whole
    and in order: its OS's flags, then its CPU's, then its OS and CPU
    pair's.
    """
    flags = fixup.rustc_flags
    common = list(flags.get("common", []))

    def on(p):
        return tuple(flag for layer in _overlays_on(flags, p) for flag in layer or [])

    per = platforms.branches(on)
    if per is None:
        return PlatformRustcFlags(common=common + list(on(next(iter(platforms)))))
    return PlatformRustcFlags(common=common, by_platform={k: list(v) for k, v in per.items()})


def get_fixup_env(fixup: CrateFixup, platforms: Platforms) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    """The env a crate's fixup gives it: common entries, and select() branches when platforms differ.

    Overlays never disagree on a key: resolving the fixup fails first.
    """
    env = fixup.env
    common = dict(env.get("common", {}))

    def on(p):
        return {k: v for layer in _overlays_on(env, p) for k, v in (layer or {}).items()}

    per = platforms.branches(on)
    if per is None:
        return {**common, **on(next(iter(platforms)))}, {}
    return common, per


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


def _escape(value: str) -> str:
    """A string's content, escaped for a Starlark double-quoted literal on one line."""
    value = value.replace("\n", " ").replace("\r", " ")
    return value.replace("\\", "\\\\").replace('"', '\\"')


def _list_literal(items: list[str], format_item) -> str:
    """A multi-line list literal, its first line unindented (it follows `name = `)."""
    lines = ["["] + [f"        {format_item(i)}," for i in items] + ["    ]"]
    return "\n".join(lines)


def _dict_literal(items: dict[str, str]) -> str:
    """A multi-line dict literal of strings, its first line unindented."""
    lines = ["{"] + [f'        "{_escape(k)}": "{_escape(v)}",' for k, v in sorted(items.items())] + ["    }"]
    return "\n".join(lines)


def _format_env_select(by_platform: dict[str, dict[str, str]], indent: str) -> str:
    """Format a select() expression for platform-specific env entries."""
    lines = [f"{indent}select({{"]
    for platform_key in sorted(by_platform.keys()):
        items = by_platform[platform_key]
        if not items:
            lines.append(f'{indent}    "{platform_key}": {{}},')
            continue
        lines.append(f'{indent}    "{platform_key}": {{')
        for key, value in sorted(items.items()):
            lines.append(f'{indent}        "{_escape(key)}": "{_escape(value)}",')
        lines.append(f"{indent}    }},")
    lines.append(f"{indent}}})")
    return "\n".join(lines)


def _host_select(host_key: str, items: list[str], format_item) -> str:
    """A select() with only the host platform's branch.

    What a fixup builds natively exists only for the platform the cell was
    built on, so configuring the crate for any other platform fails here
    ("no condition matched") instead of linking a missing library.
    """
    lines = ["    select({", f'        "{host_key}": [']
    lines += [f"            {format_item(i)}," for i in items]
    lines += ["        ],", "    })"]
    return "\n".join(lines)


def _attr(lines: list[str], name: str, parts: list[str], operator: str = "+") -> None:
    """Append `name = part1 <op> part2 ...,` when there are parts; each part's first line is unindented."""
    if not parts:
        return
    lines.append(f"    {name} = " + f" {operator}\n    ".join(part.lstrip() for part in parts) + ",")


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
    native_libraries: list[dict] | None = None,
    host_key: str | None = None,
    env_by_platform: dict[str, dict[str, str]] | None = None,
) -> str:
    """Generate BUCK file content.

    native_libraries are the fixup's pre-built libraries (NativeLibrarySpec
    dicts); they are linked only on host_key, the combined config_setting
    of the platform the cell was built on.
    """
    rules_to_load = ["rust_library"]
    native_lib_content = []
    native_deps: list[str] = []
    native_flags: list[str] = []

    for info in native_libraries or []:
        generated = buck2_generator.generate(NativeLibrarySpec.from_dict(info))
        rules_to_load.extend(r for r in generated.rules_to_load if r not in rules_to_load)
        native_lib_content.append(generated.rules_content)
        native_deps += generated.extra_deps
        native_flags += generated.extra_rustc_flags
    if native_deps and host_key is None:
        raise ValueError(f"{crate_name}: native libraries need the host platform's key")

    rules_str = ", ".join(f'"{r}"' for r in rules_to_load)
    lines = [
        "# Auto-generated by turnkey rust-deps-cell",
        f'load("@prelude//:rules.bzl", {rules_str})',
        "",
    ]
    lines.extend(native_lib_content)

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

    quoted = lambda d: f'"{d}"'

    # Deps: common, then per platform, then the host's native libraries
    deps = []
    if platform_deps.common:
        deps.append(_list_literal(sorted(set(platform_deps.common)), quoted))
    if platform_deps.by_platform:
        deps.append(_format_select(platform_deps.by_platform, "    ", quoted))
    if native_deps:
        deps.append(_host_select(host_key, native_deps, quoted))
    _attr(lines, "deps", deps)

    # Named deps: common + optional select() for platform-specific
    named = []
    if platform_named_deps.common:
        named.append(_dict_literal(platform_named_deps.common))
    if platform_named_deps.by_platform:
        named.append(_format_named_select(platform_named_deps.by_platform, "    "))
    _attr(lines, "named_deps", named, operator="|")

    # Cargo's and the fixup's environment variables
    envs = []
    if env:
        envs.append(_dict_literal(env))
    if env_by_platform:
        envs.append(_format_env_select(env_by_platform, "    "))
    _attr(lines, "env", envs, operator="|")

    # Rustc flags (build script cfg emulation), kept in order: they come in pairs
    flag = lambda f: f'"{_escape(f)}"'
    flags = []
    if rustc_flags.common:
        flags.append(_list_literal(rustc_flags.common, flag))
    if rustc_flags.by_platform:
        flags.append(_format_select(rustc_flags.by_platform, "    ", flag, dedup_sort=False))
    if native_flags:
        flags.append(_host_select(host_key, native_flags, flag))
    _attr(lines, "rustc_flags", flags)

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
