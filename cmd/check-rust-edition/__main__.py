#!/usr/bin/env python3
"""Check that Rust editions are consistent between Cargo.toml and rules.star files.

This script verifies:
1. All workspace members use edition.workspace = true
2. rules.star files have edition matching workspace.package.edition
"""

import sys
import re
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib


def parse_toml(path: Path) -> dict:
    """Parse a TOML file."""
    with open(path, "rb") as f:
        return tomllib.load(f)


def extract_buck_editions(buck_path: Path) -> list[tuple[str, str]]:
    """Extract edition values from a rules.star file.

    Returns list of (target_name, edition) tuples.
    """
    content = buck_path.read_text()
    editions = []

    # Match patterns like: edition = "2024",
    # We also try to find the target name from preceding lines
    lines = content.split('\n')
    current_target = None

    for i, line in enumerate(lines):
        # Look for target definitions
        if 'rust_binary(' in line or 'rust_library(' in line or 'rust_test(' in line:
            current_target = line.strip()

        # Look for name = "..."
        name_match = re.search(r'name\s*=\s*"([^"]+)"', line)
        if name_match:
            current_target = name_match.group(1)

        # Look for edition = "..."
        edition_match = re.search(r'edition\s*=\s*"([^"]+)"', line)
        if edition_match:
            editions.append((current_target or "unknown", edition_match.group(1)))

    return editions


def check_workspace_member(
    member_path: Path,
    workspace_edition: str,
    errors: list[str],
) -> None:
    """Check a workspace member's Cargo.toml and rules.star file."""
    cargo_toml = member_path / "Cargo.toml"
    buck_file = member_path / "rules.star"

    if not cargo_toml.exists():
        return

    cargo = parse_toml(cargo_toml)
    package = cargo.get("package", {})
    edition = package.get("edition")

    # Check if using workspace inheritance
    if isinstance(edition, dict):
        if not edition.get("workspace"):
            errors.append(
                f"{cargo_toml}: edition should use 'edition.workspace = true'"
            )
    elif isinstance(edition, str):
        errors.append(
            f"{cargo_toml}: should use 'edition.workspace = true' instead of "
            f"'edition = \"{edition}\"'"
        )
    elif edition is None:
        # No edition specified - Cargo defaults to 2015, but this is unusual
        errors.append(
            f"{cargo_toml}: no edition specified, should use 'edition.workspace = true'"
        )

    # Check rules.star file if it exists
    if buck_file.exists():
        buck_editions = extract_buck_editions(buck_file)
        for target_name, buck_edition in buck_editions:
            if buck_edition != workspace_edition:
                errors.append(
                    f"{buck_file}: target '{target_name}' has edition = \"{buck_edition}\", "
                    f"expected \"{workspace_edition}\" (from workspace.package.edition)"
                )


def main() -> int:
    """Main entry point."""
    # Find project root (directory with Cargo.toml containing [workspace])
    cwd = Path.cwd()
    root = cwd

    while root != root.parent:
        cargo_toml = root / "Cargo.toml"
        if cargo_toml.exists():
            try:
                cargo = parse_toml(cargo_toml)
                if "workspace" in cargo:
                    break
            except Exception:
                pass
        root = root.parent
    else:
        print("Error: Could not find workspace root (Cargo.toml with [workspace])")
        return 1

    cargo_toml = root / "Cargo.toml"
    cargo = parse_toml(cargo_toml)

    # Get workspace edition
    workspace_package = cargo.get("workspace", {}).get("package", {})
    workspace_edition = workspace_package.get("edition")

    if not workspace_edition:
        print(f"Error: {cargo_toml} missing [workspace.package] edition")
        return 1

    print(f"Workspace edition: {workspace_edition}")

    # Get workspace members
    workspace = cargo.get("workspace", {})
    members = workspace.get("members", [])

    if not members:
        print("No workspace members found")
        return 0

    errors: list[str] = []

    # Check each member
    for member_pattern in members:
        # Handle glob patterns like "cmd/*"
        if "*" in member_pattern:
            base_path = root / member_pattern.replace("/*", "").replace("*", "")
            if base_path.exists():
                for member_path in base_path.iterdir():
                    if member_path.is_dir() and (member_path / "Cargo.toml").exists():
                        check_workspace_member(member_path, workspace_edition, errors)
        else:
            member_path = root / member_pattern
            if member_path.exists():
                check_workspace_member(member_path, workspace_edition, errors)

    # Report results
    if errors:
        print(f"\nFound {len(errors)} edition alignment issue(s):\n")
        for error in errors:
            print(f"  - {error}")
        print("\nTo fix:")
        print("  1. Update Cargo.toml files to use 'edition.workspace = true'")
        print(f"  2. Update rules.star files to use 'edition = \"{workspace_edition}\"'")
        return 1

    print("All editions are aligned")
    return 0


if __name__ == "__main__":
    sys.exit(main())
