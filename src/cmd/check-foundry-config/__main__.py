#!/usr/bin/env python3
"""Check Foundry configuration consistency across the monorepo.

This script verifies that:
1. solc_version in foundry.toml files matches the toolchain's solc version
2. Dependencies in per-project foundry.toml are subsets of root foundry.toml
3. Dependency versions match exactly

This ensures native `forge` commands work correctly with the toolchain-provided solc
and that dependency declarations stay synchronized.
"""

import re
import subprocess
import sys
import tomllib
from pathlib import Path


def find_project_root() -> Path:
    """Find the project root (directory with .git or .buckroot)."""
    cwd = Path.cwd()
    root = cwd

    while root != root.parent:
        if (root / ".git").exists() or (root / ".buckroot").exists():
            return root
        root = root.parent

    return cwd


def get_toolchain_solc_version() -> str | None:
    """Get the solc version from the toolchain by running solc --version."""
    try:
        result = subprocess.run(
            ["solc", "--version"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if result.returncode != 0:
            return None

        # Parse version from output like:
        # "Version: 0.8.33+commit.64118f21.Linux.g++"
        for line in result.stdout.split("\n"):
            if line.startswith("Version:"):
                # Extract just the version number (0.8.33)
                match = re.search(r"(\d+\.\d+\.\d+)", line)
                if match:
                    return match.group(1)
        return None
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None


def parse_foundry_toml(path: Path) -> dict | None:
    """Parse a foundry.toml file and return its contents."""
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except (tomllib.TOMLDecodeError, OSError) as e:
        print(f"  Error parsing {path}: {e}")
        return None


def get_solc_version_from_config(config: dict) -> str | None:
    """Extract solc_version from foundry config (checking all profiles)."""
    # Check profile.default first
    if "profile" in config:
        for profile_name, profile in config["profile"].items():
            if "solc_version" in profile:
                return profile["solc_version"]

    # Check top-level (older format)
    return config.get("solc_version")


def get_dependencies_from_config(config: dict) -> dict[str, str]:
    """Extract dependencies from foundry config."""
    return config.get("dependencies", {})


def find_foundry_configs(root: Path) -> list[Path]:
    """Find all foundry.toml files in the repository."""
    configs = []

    for path in root.rglob("foundry.toml"):
        # Skip hidden directories, node_modules, buck-out, etc.
        parts = path.relative_to(root).parts
        if any(
            p in ("node_modules", "buck-out", ".git", ".devenv", ".turnkey", "target")
            or p.startswith(".")
            for p in parts
        ):
            continue
        configs.append(path)

    return sorted(configs)


def check_solc_version(
    config_path: Path,
    config: dict,
    toolchain_version: str | None,
    errors: list[str],
    warnings: list[str],
) -> None:
    """Check that solc_version matches the toolchain."""
    config_version = get_solc_version_from_config(config)

    if config_version is None:
        # No version specified - that's fine, forge will use default
        return

    if toolchain_version is None:
        warnings.append(
            f"{config_path}: specifies solc_version = {config_version!r} "
            "but solc is not available in PATH to verify"
        )
        return

    if config_version != toolchain_version:
        errors.append(
            f"{config_path}: solc_version = {config_version!r} "
            f"but toolchain provides {toolchain_version!r}"
        )


def check_dependencies(
    config_path: Path,
    config: dict,
    root_deps: dict[str, str],
    errors: list[str],
) -> None:
    """Check that dependencies are subset of root and versions match."""
    config_deps = get_dependencies_from_config(config)

    for dep_name, dep_spec in config_deps.items():
        if dep_name not in root_deps:
            errors.append(
                f"{config_path}: dependency {dep_name!r} not declared in root foundry.toml"
            )
        elif root_deps[dep_name] != dep_spec:
            errors.append(
                f"{config_path}: dependency {dep_name!r} = {dep_spec!r} "
                f"but root has {root_deps[dep_name]!r}"
            )


def main() -> int:
    """Main entry point."""
    root = find_project_root()
    errors: list[str] = []
    warnings: list[str] = []

    print(f"Checking Foundry configuration consistency in {root}...")
    print()

    # Get toolchain solc version
    toolchain_solc = get_toolchain_solc_version()
    if toolchain_solc:
        print(f"Toolchain solc version: {toolchain_solc}")
    else:
        print("Warning: Could not determine toolchain solc version (solc not in PATH)")
    print()

    # Find all foundry.toml files
    configs = find_foundry_configs(root)
    if not configs:
        print("No foundry.toml files found.")
        return 0

    # Find root config
    root_config_path = root / "foundry.toml"
    if root_config_path not in configs:
        print("No root foundry.toml found, skipping dependency checks.")
        root_deps = {}
        root_config = None
    else:
        root_config = parse_foundry_toml(root_config_path)
        if root_config is None:
            print(f"Error: Could not parse root {root_config_path}")
            return 1
        root_deps = get_dependencies_from_config(root_config)
        print(f"Root foundry.toml has {len(root_deps)} dependencies")

    print(f"Found {len(configs)} foundry.toml file(s)")
    print()

    # Check each config
    for config_path in configs:
        config = parse_foundry_toml(config_path)
        if config is None:
            errors.append(f"{config_path}: failed to parse")
            continue

        rel_path = config_path.relative_to(root)
        print(f"Checking {rel_path}...")

        # Check solc version
        check_solc_version(config_path, config, toolchain_solc, errors, warnings)

        # Check dependencies (skip root config)
        if config_path != root_config_path:
            check_dependencies(config_path, config, root_deps, errors)

    print()

    # Report warnings
    if warnings:
        print(f"Warnings ({len(warnings)}):")
        for warning in warnings:
            print(f"  - {warning}")
        print()

    # Report errors
    if errors:
        print(f"Errors ({len(errors)}):")
        for error in errors:
            print(f"  - {error}")
        print()
        print("Fix: Update foundry.toml files to match toolchain and root config:")
        print()
        if toolchain_solc:
            print(f"  solc_version = \"{toolchain_solc}\"")
        print()
        print("  Dependencies should be declared in root foundry.toml only,")
        print("  or match exactly if duplicated in per-project configs.")
        return 1

    print("All Foundry configurations are consistent.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
