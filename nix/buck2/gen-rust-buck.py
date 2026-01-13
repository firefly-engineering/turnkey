#!/usr/bin/env python3
"""
Generate BUCK files for Rust crates by parsing their Cargo.toml.

This script reads a crate's Cargo.toml and generates a Buck2 BUCK file
with proper dependencies, crate_root detection, and file globs.
"""

import json
import os
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Union


# =============================================================================
# cfg() Expression Parser
# =============================================================================
# Parses Cargo's cfg() expressions and evaluates them against a target triple.
#
# Grammar:
#   cfg_expr    = "cfg(" predicate ")"
#   predicate   = key | key "=" value | "all(" pred_list ")" | "any(" pred_list ")" | "not(" predicate ")"
#   pred_list   = predicate ("," predicate)*
#   key         = identifier
#   value       = quoted_string
#
# Reference: https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies
# =============================================================================


@dataclass
class CfgKey:
    """A simple key predicate like 'unix' or 'windows'."""

    key: str


@dataclass
class CfgKeyValue:
    """A key-value predicate like 'target_os = "linux"'."""

    key: str
    value: str


@dataclass
class CfgAll:
    """An all(...) combinator - true if all children are true."""

    children: list["CfgPredicate"]


@dataclass
class CfgAny:
    """An any(...) combinator - true if any child is true."""

    children: list["CfgPredicate"]


@dataclass
class CfgNot:
    """A not(...) combinator - negates the child."""

    child: "CfgPredicate"


CfgPredicate = Union[CfgKey, CfgKeyValue, CfgAll, CfgAny, CfgNot]


class CfgParser:
    """Parser for cfg() expressions."""

    def __init__(self, text: str):
        self.text = text
        self.pos = 0

    def parse(self) -> CfgPredicate | None:
        """Parse a cfg() expression. Returns None if not a valid cfg expression."""
        self.skip_whitespace()

        # Check for cfg( prefix
        if not self.text.lower().startswith("cfg("):
            return None

        self.pos = 4  # Skip "cfg("
        predicate = self.parse_predicate()

        self.skip_whitespace()
        if self.pos < len(self.text) and self.text[self.pos] == ")":
            self.pos += 1

        return predicate

    def parse_predicate(self) -> CfgPredicate | None:
        """Parse a single predicate."""
        self.skip_whitespace()

        if self.pos >= len(self.text):
            return None

        # Check for combinators
        remaining = self.text[self.pos :].lower()

        if remaining.startswith("all("):
            self.pos += 4
            children = self.parse_predicate_list()
            self.expect(")")
            return CfgAll(children)

        if remaining.startswith("any("):
            self.pos += 4
            children = self.parse_predicate_list()
            self.expect(")")
            return CfgAny(children)

        if remaining.startswith("not("):
            self.pos += 4
            child = self.parse_predicate()
            self.expect(")")
            return CfgNot(child) if child else None

        # Parse key or key = value
        return self.parse_key_or_key_value()

    def parse_predicate_list(self) -> list[CfgPredicate]:
        """Parse a comma-separated list of predicates."""
        predicates = []

        while True:
            self.skip_whitespace()
            if self.pos >= len(self.text) or self.text[self.pos] == ")":
                break

            pred = self.parse_predicate()
            if pred:
                predicates.append(pred)

            self.skip_whitespace()
            if self.pos < len(self.text) and self.text[self.pos] == ",":
                self.pos += 1  # Skip comma
            else:
                break

        return predicates

    def parse_key_or_key_value(self) -> CfgPredicate | None:
        """Parse either a key or a key = value pair."""
        self.skip_whitespace()
        key = self.parse_identifier()

        if not key:
            return None

        self.skip_whitespace()

        # Check for = value
        if self.pos < len(self.text) and self.text[self.pos] == "=":
            self.pos += 1  # Skip =
            self.skip_whitespace()
            value = self.parse_string()
            return CfgKeyValue(key, value) if value else CfgKey(key)

        return CfgKey(key)

    def parse_identifier(self) -> str | None:
        """Parse an identifier (alphanumeric + underscores)."""
        self.skip_whitespace()
        match = re.match(r"[a-zA-Z_][a-zA-Z0-9_]*", self.text[self.pos :])
        if match:
            self.pos += match.end()
            return match.group()
        return None

    def parse_string(self) -> str | None:
        """Parse a quoted string."""
        self.skip_whitespace()
        if self.pos >= len(self.text):
            return None

        quote = self.text[self.pos]
        if quote not in ('"', "'"):
            return None

        self.pos += 1  # Skip opening quote
        end = self.text.find(quote, self.pos)
        if end == -1:
            return None

        value = self.text[self.pos : end]
        self.pos = end + 1
        return value

    def skip_whitespace(self):
        """Skip whitespace characters."""
        while self.pos < len(self.text) and self.text[self.pos] in " \t\n\r":
            self.pos += 1

    def expect(self, char: str):
        """Expect and consume a specific character."""
        self.skip_whitespace()
        if self.pos < len(self.text) and self.text[self.pos] == char:
            self.pos += 1


@dataclass
class TargetSpec:
    """A target specification (e.g., x86_64-unknown-linux-gnu)."""

    arch: str
    vendor: str
    os: str
    env: str | None
    family: str

    @classmethod
    def linux_x86_64(cls) -> "TargetSpec":
        """Create a spec for x86_64-unknown-linux-gnu."""
        return cls(
            arch="x86_64",
            vendor="unknown",
            os="linux",
            env="gnu",
            family="unix",
        )


def evaluate_cfg(predicate: CfgPredicate, target: TargetSpec) -> bool:
    """Evaluate a cfg predicate against a target specification."""
    if isinstance(predicate, CfgKey):
        # Handle shorthand keys
        key = predicate.key.lower()
        if key == "unix":
            return target.family == "unix"
        if key == "windows":
            return target.family == "windows"
        # Unknown key - assume true (be permissive)
        return True

    if isinstance(predicate, CfgKeyValue):
        key = predicate.key.lower()
        value = predicate.value.lower()

        if key == "target_os":
            return target.os.lower() == value
        if key == "target_arch":
            return target.arch.lower() == value
        if key == "target_family":
            return target.family.lower() == value
        if key == "target_vendor":
            return target.vendor.lower() == value
        if key == "target_env":
            return (target.env or "").lower() == value
        if key == "target_pointer_width":
            return (value == "64" and target.arch in ("x86_64", "aarch64")) or (
                value == "32" and target.arch in ("x86", "arm")
            )
        if key == "target_endian":
            # Most common architectures are little-endian
            return value == "little"
        if key == "feature":
            # Features are handled separately
            return True

        # Unknown key - assume true (be permissive)
        return True

    if isinstance(predicate, CfgAll):
        return all(evaluate_cfg(child, target) for child in predicate.children)

    if isinstance(predicate, CfgAny):
        # Empty any() is false
        if not predicate.children:
            return False
        return any(evaluate_cfg(child, target) for child in predicate.children)

    if isinstance(predicate, CfgNot):
        return not evaluate_cfg(predicate.child, target)

    return True


def is_linux_compatible_target(target_spec: str) -> bool:
    """Check if a target specification is compatible with Linux x86_64.

    Uses proper cfg() expression parsing for complex expressions.
    Falls back to string matching for non-cfg expressions.
    """
    target_spec = target_spec.strip()

    # Try parsing as cfg() expression
    parser = CfgParser(target_spec)
    predicate = parser.parse()

    if predicate:
        target = TargetSpec.linux_x86_64()
        return evaluate_cfg(predicate, target)

    # Fallback for non-cfg expressions (e.g., direct target triples)
    target = target_spec.lower()

    # Direct target triple matching
    if "linux" in target or "x86_64-unknown-linux" in target:
        return True

    # Exclude non-Linux targets
    if any(
        os in target
        for os in ["windows", "darwin", "macos", "ios", "android", "wasm", "wasi"]
    ):
        return False

    # Unknown - assume compatible
    return True


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


def get_build_script_cfg_flags(crate_name: str, version: str, registry: dict) -> list[str]:
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
            f'    preferred_linkage = "static",',
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
        print("Usage: gen-rust-buck.py <crate_dir> <available_crates_json> [fixup_crates_json] [unified_features_json] [rustc_flags_registry_json]", file=sys.stderr)
        sys.exit(1)

    crate_dir = Path(sys.argv[1])
    available_crates = set(json.loads(sys.argv[2]))
    fixup_crates = set(json.loads(sys.argv[3])) if len(sys.argv) > 3 else set()
    unified_features = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
    rustc_flags_registry = json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}

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
    rustc_flags = get_build_script_cfg_flags(crate_name, version, rustc_flags_registry)

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
