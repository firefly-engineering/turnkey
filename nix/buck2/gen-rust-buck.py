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
    """Determine the library crate root path."""
    # Check explicit [lib] path
    lib_section = cargo.get("lib", {})
    if "path" in lib_section:
        return lib_section["path"]

    # Check for standard locations
    src_lib = crate_dir / "src" / "lib.rs"
    if src_lib.exists():
        # Check for ambiguous case (multiple lib.rs files)
        core_lib = crate_dir / "src" / "core" / "lib.rs"
        if core_lib.exists():
            # Prefer src/lib.rs
            return "src/lib.rs"
        return None  # Default, no need to specify

    lib_rs = crate_dir / "lib.rs"
    if lib_rs.exists():
        return "lib.rs"

    return None


def is_proc_macro(cargo: dict) -> bool:
    """Check if the crate is a proc-macro crate."""
    return cargo.get("lib", {}).get("proc-macro", False)


def get_cargo_env(cargo: dict, crate_name: str) -> dict[str, str]:
    """Get Cargo environment variables that should be set during build.

    Cargo sets several environment variables that crates can access via env!().
    We replicate the most commonly used ones.
    """
    pkg = cargo.get("package", {})
    env = {
        "CARGO_PKG_NAME": crate_name,
        "CARGO_PKG_VERSION": pkg.get("version", "0.0.0"),
        "CARGO_PKG_VERSION_MAJOR": pkg.get("version", "0.0.0").split(".")[0],
        "CARGO_PKG_VERSION_MINOR": pkg.get("version", "0.0.0").split(".")[1] if "." in pkg.get("version", "0.0.0") else "0",
        "CARGO_PKG_VERSION_PATCH": pkg.get("version", "0.0.0").split(".")[2].split("-")[0] if pkg.get("version", "0.0.0").count(".") >= 2 else "0",
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


def resolve_dep(pkg_name: str, available_crates: set[str]) -> str | None:
    """Resolve a package name to a Buck target if it exists in available crates."""
    # Crate names in Cargo use hyphens or underscores, check both forms
    if pkg_name in available_crates:
        return f'rustdeps//vendor/{pkg_name}:{pkg_name}'
    elif pkg_name.replace("-", "_") in available_crates:
        normalized = pkg_name.replace("-", "_")
        return f'rustdeps//vendor/{normalized}:{normalized}'
    elif pkg_name.replace("_", "-") in available_crates:
        normalized = pkg_name.replace("_", "-")
        return f'rustdeps//vendor/{normalized}:{normalized}'
    return None


def extract_deps_from_section(section_deps: dict, available_crates: set[str]) -> list[str]:
    """Extract dependencies from a Cargo.toml dependency section."""
    deps = []
    for dep_name, dep_spec in section_deps.items():
        # Get the actual package name (may be different from dependency key)
        if isinstance(dep_spec, dict) and "package" in dep_spec:
            pkg_name = dep_spec["package"]
        else:
            pkg_name = dep_name

        target = resolve_dep(pkg_name, available_crates)
        if target:
            deps.append(target)
    return deps


def is_linux_compatible_target(target_spec: str) -> bool:
    """Check if a target specification is compatible with Linux.

    Handles common cfg() expressions from Cargo.toml.
    """
    target = target_spec.lower()

    # Skip Windows-only targets
    if "windows" in target:
        return False

    # Skip targets that explicitly exclude Unix
    if "not(unix)" in target:
        return False

    # Skip macOS-only targets (darwin, macos)
    if "target_os" in target and ("macos" in target or "darwin" in target):
        # But not if it's a general Unix target that includes macos
        if "unix" not in target:
            return False

    # Skip other non-Linux OS targets
    if any(os in target for os in ["redox", "wasi", "ios", "android", "freebsd", "openbsd", "netbsd"]):
        return False

    # Include Unix targets (which includes Linux)
    if "unix" in target:
        return True

    # Include Linux-specific targets
    if "linux" in target:
        return True

    # Include targets that are platform-agnostic feature flags
    if "target_os" not in target and "target_family" not in target:
        return True

    return True


def get_dependencies(cargo: dict, available_crates: set[str]) -> list[str]:
    """Extract dependencies that exist in our vendored crates.

    Note: We only include regular dependencies, not build-dependencies.
    Build scripts require separate rust_build_script rules in Buck2.
    Also, we only include dependencies compatible with Linux.
    """
    deps = []

    # Standard dependencies (not build-dependencies)
    section_deps = cargo.get("dependencies", {})
    deps.extend(extract_deps_from_section(section_deps, available_crates))

    # Target-specific dependencies - only for Linux-compatible targets
    for target_spec, target_config in cargo.get("target", {}).items():
        if is_linux_compatible_target(target_spec):
            section_deps = target_config.get("dependencies", {})
            deps.extend(extract_deps_from_section(section_deps, available_crates))

    return deps


def generate_buck_file(
    crate_name: str,
    edition: str,
    crate_root: str | None,
    deps: list[str],
    proc_macro: bool,
    env: dict[str, str],
) -> str:
    """Generate BUCK file content."""
    lines = [
        "# Auto-generated by turnkey rust-deps-cell",
        'load("@prelude//:rules.bzl", "rust_library")',
        "",
        "rust_library(",
        f'    name = "{crate_name}",',
        '    srcs = glob(["**/*"]),',
        f'    edition = "{edition}",',
    ]

    if proc_macro:
        lines.append('    proc_macro = True,')

    if crate_root:
        lines.append(f'    crate_root = "{crate_root}",')

    if deps:
        lines.append("    deps = [")
        for dep in sorted(set(deps)):
            lines.append(f'        "{dep}",')
        lines.append("    ],")

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

    lines.extend([
        '    visibility = ["PUBLIC"],',
        ")",
        "",
    ])

    return "\n".join(lines)


def main():
    if len(sys.argv) < 3:
        print("Usage: gen-rust-buck.py <crate_dir> <available_crates_json>", file=sys.stderr)
        sys.exit(1)

    crate_dir = Path(sys.argv[1])
    available_crates = set(json.loads(sys.argv[2]))

    # Get crate name from directory (format: name@version or just name)
    dir_name = crate_dir.name
    if "@" in dir_name:
        fallback_name = dir_name.split("@")[0]
    else:
        fallback_name = dir_name

    cargo = parse_cargo_toml(crate_dir)
    crate_name = get_crate_name(cargo, fallback_name)
    edition = get_edition(cargo)
    crate_root = get_lib_path(cargo, crate_dir)
    deps = get_dependencies(cargo, available_crates)
    proc_macro = is_proc_macro(cargo)
    env = get_cargo_env(cargo, crate_name)

    buck_content = generate_buck_file(crate_name, edition, crate_root, deps, proc_macro, env)
    print(buck_content)


if __name__ == "__main__":
    main()
