#!/usr/bin/env python3
"""
Generate BUCK files for Rust crates by parsing their Cargo.toml.

This script reads a crate's Cargo.toml and generates a Buck2 BUCK file
with proper dependencies, crate_root detection, and file globs.
"""

import json
import os
import sys
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


def dep_is_available(dep_name: str, available_crates: set[str]) -> bool:
    """Check if a dependency is available (checking hyphen/underscore variants)."""
    return (dep_name in available_crates or
            dep_name.replace("-", "_") in available_crates or
            dep_name.replace("_", "-") in available_crates)


def feature_enables_unavailable_dep(feature_name: str, features: dict, available_crates: set[str]) -> bool:
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
    # 3. Features that enable unavailable deps via dep: syntax (e.g., default-hasher = ["dep:foldhash"])
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
        env["CARGO_PKG_AUTHORS"] = ":".join(pkg["authors"]) if isinstance(pkg["authors"], list) else pkg["authors"]

    return env


def find_matching_version(pkg_name: str, version_req: str | None, available_crates: set[str]) -> str | None:
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
        if name != pkg_name and name != pkg_name.replace("-", "_") and name != pkg_name.replace("_", "-"):
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
    candidates.sort(key=lambda x: [int(p) for p in x[1].split(".")[:3] if p.isdigit()], reverse=True)
    return candidates[0][0]


def resolve_dep(pkg_name: str, available_crates: set[str], version_req: str | None = None) -> str | None:
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
        return f'rustdeps//vendor/{pkg_name}:{pkg_name}'
    elif pkg_name.replace("-", "_") in available_crates:
        normalized = pkg_name.replace("-", "_")
        return f'rustdeps//vendor/{normalized}:{normalized}'
    elif pkg_name.replace("_", "-") in available_crates:
        normalized = pkg_name.replace("_", "-")
        return f'rustdeps//vendor/{normalized}:{normalized}'
    return None


def normalize_crate_name(name: str) -> str:
    """Normalize crate name (Cargo treats hyphens and underscores as equivalent)."""
    return name.replace("-", "_")


def get_version_req(dep_spec) -> str | None:
    """Extract version requirement from a dependency specification."""
    if isinstance(dep_spec, str):
        return dep_spec
    elif isinstance(dep_spec, dict):
        return dep_spec.get("version")
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


def is_linux_compatible_target(target_spec: str) -> bool:
    """Check if a target specification is compatible with Linux x86_64.

    Handles common cfg() expressions from Cargo.toml.
    For cfg(any(...)) expressions, we include if ANY condition could match Linux.
    """
    target = target_spec.lower()

    # cfg(any()) with empty parens means "never match any target"
    if "cfg(any())" in target:
        return False

    # Skip targets that explicitly exclude Unix
    if "not(unix)" in target:
        return False

    # If target includes "unix" or "linux", it's compatible with Linux
    # Check this BEFORE checking exclusions, since cfg(any(unix, wasi)) should match
    if "unix" in target or "linux" in target:
        return True

    # Skip wasm32/wasm64-only targets (but not if they also include unix)
    if "wasm32" in target or "wasm64" in target:
        return False

    # Skip Windows-only targets
    if "windows" in target:
        return False

    # Skip macOS-only targets (darwin, macos)
    if "target_os" in target and ("macos" in target or "darwin" in target):
        return False

    # Skip other non-Linux OS targets (only if unix not in target)
    if any(os in target for os in ["redox", "wasi", "ios", "android", "freebsd", "openbsd", "netbsd", "uefi"]):
        return False

    # Include targets that are platform-agnostic feature flags
    if "target_os" not in target and "target_family" not in target:
        return True

    return True


def get_dependencies(cargo: dict, available_crates: set[str]) -> tuple[list[str], dict[str, str]]:
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
    section_deps_list, section_named = extract_deps_from_section(section_deps, available_crates)
    deps.extend(section_deps_list)
    named_deps.update(section_named)

    # Target-specific dependencies - only for Linux-compatible targets
    for target_spec, target_config in cargo.get("target", {}).items():
        if is_linux_compatible_target(target_spec):
            section_deps = target_config.get("dependencies", {})
            section_deps_list, section_named = extract_deps_from_section(section_deps, available_crates)
            deps.extend(section_deps_list)
            named_deps.update(section_named)

    return deps, named_deps


def get_build_script_cfg_flags(crate_name: str) -> list[str]:
    """Get rustc cfg flags that would be set by a crate's build script.

    Some crates have build scripts that probe the target and emit
    cargo:rustc-cfg directives. We hardcode these for known crates
    since we can't run build scripts in the Nix sandbox.

    These are x86_64-linux specific for now.
    """
    if crate_name == "serde_json":
        # serde_json's build.rs sets fast_arithmetic based on target arch
        # On x86_64, it uses 64-bit arithmetic
        # Note: Quotes need escaping for BUCK file output
        return ['--cfg', 'fast_arithmetic=\\"64\\"']
    if crate_name == "rustix":
        # rustix's build.rs selects backend and platform features based on target
        # For x86_64-linux with libc backend, we need:
        # - libc: selects the libc backend (vs linux_raw which requires more setup)
        # - linux_like: enables Linux-like OS features
        # - linux_kernel: enables Linux-specific syscalls like sendfile
        # See: https://github.com/bytecodealliance/rustix/blob/main/build.rs
        return ['--cfg', 'libc', '--cfg', 'linux_like', '--cfg', 'linux_kernel']
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
        lib_name = f"ring_core_0_17_{patch}_"
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
        lines.extend([
            f"# Native crypto library pre-compiled in Nix",
            f"export_file(",
            f'    name = "{lib_name}_file",',
            f'    src = "{static_lib_path}",',
            f'    visibility = ["PUBLIC"],',
            f")",
            "",
            f"prebuilt_cxx_library(",
            f'    name = "{lib_name}",',
            f'    static_lib = ":{lib_name}_file",',
            f'    link_whole = True,',
            f'    visibility = ["PUBLIC"],',
            f")",
            "",
        ])
        # Add the native library as a dependency
        deps = deps + [f":{lib_name}"]
        # Add -L flag so rustc can find the library during compilation
        rustc_flags = rustc_flags + [f"-Lnative={link_search_path}"]

    lines.extend([
        "rust_library(",
        f'    name = "{crate_name}",',
        '    srcs = glob(["**/*"]),',
        f'    edition = "{edition}",',
    ])

    if proc_macro:
        lines.append('    proc_macro = True,')

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

    # Add named_deps for renamed dependencies (e.g., pki-types = { package = "rustls-pki-types" })
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
            escaped_value = value.replace('\n', ' ').replace('\r', ' ')
            escaped_value = escaped_value.replace('\\', '\\\\').replace('"', '\\"')
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

    lines.extend([
        '    visibility = ["PUBLIC"],',
        ")",
        "",
    ])

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


def main():
    if len(sys.argv) < 3:
        print("Usage: gen-rust-buck.py <crate_dir> <available_crates_json> [fixup_crates_json] [unified_features_json]", file=sys.stderr)
        sys.exit(1)

    crate_dir = Path(sys.argv[1])
    available_crates = set(json.loads(sys.argv[2]))
    fixup_crates = set(json.loads(sys.argv[3])) if len(sys.argv) > 3 else set()
    unified_features = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}

    # Get crate name from directory (format: name@version or just name)
    dir_name = crate_dir.name
    if "@" in dir_name:
        fallback_name = dir_name.split("@")[0]
    else:
        fallback_name = dir_name

    cargo = parse_cargo_toml(crate_dir)
    crate_name = get_crate_name(cargo, fallback_name)
    version = cargo.get("package", {}).get("version", "0.0.0")
    edition = get_edition(cargo)
    crate_root = get_lib_path(cargo, crate_dir)
    deps, named_deps = get_dependencies(cargo, available_crates)
    proc_macro = is_proc_macro(cargo)
    env = get_cargo_env(cargo, crate_name)
    rustc_flags = get_build_script_cfg_flags(crate_name)

    # Get native library info for crates with pre-built native code
    native_lib_info = get_native_library_info(crate_name, version)

    # Use unified features if available, otherwise fall back to default features
    if crate_name in unified_features:
        features = unified_features[crate_name]
        # Still need to filter for availability (unified features may include
        # features that enable deps we don't have)
        features = filter_features_for_availability(features, cargo, available_crates)
    else:
        features = get_default_features(cargo, available_crates)

    # Add OUT_DIR for crates that have build script fixups
    if crate_name in fixup_crates:
        env["OUT_DIR"] = "out_dir"

    buck_content = generate_buck_file(
        crate_name, edition, crate_root, deps, named_deps,
        proc_macro, features, env, rustc_flags, native_lib_info
    )
    print(buck_content)


if __name__ == "__main__":
    main()
