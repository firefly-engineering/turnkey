#!/usr/bin/env python3
"""Check that monorepo dependency rules are followed for all languages.

This script verifies:

Go:
- No nested go.mod files in subdirectories (single module at root)

Rust:
- All workspace member deps use workspace = true references
- No direct version specs for deps that exist in workspace.dependencies

Python:
- No nested pyproject.toml files with their own [project.dependencies]
- All deps should be in root pyproject.toml

TypeScript/JavaScript:
- No nested package.json with direct deps (should use workspace: protocol)
- All deps should be in root package.json
"""

import json
import sys
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib


def parse_toml(path: Path) -> dict:
    """Parse a TOML file."""
    with open(path, "rb") as f:
        return tomllib.load(f)


def parse_json(path: Path) -> dict:
    """Parse a JSON file."""
    with open(path, "r") as f:
        return json.load(f)


def find_project_root() -> Path:
    """Find the project root (directory with .git or .buckroot)."""
    cwd = Path.cwd()
    root = cwd

    while root != root.parent:
        if (root / ".git").exists() or (root / ".buckroot").exists():
            return root
        root = root.parent

    return cwd


def check_go(root: Path, errors: list[str]) -> None:
    """Check Go monorepo rules."""
    root_go_mod = root / "go.mod"
    if not root_go_mod.exists():
        return  # No Go in this project

    # Find all go.mod files
    for go_mod in root.rglob("go.mod"):
        if go_mod == root_go_mod:
            continue

        # Skip vendor directories, build outputs, and test fixtures
        rel_path = go_mod.relative_to(root)
        parts = rel_path.parts
        if any(p in ("vendor", "node_modules", "buck-out", ".git", "cells", "testdata") for p in parts):
            continue

        # Skip e2e fixtures (they're intentionally separate)
        if "e2e" in parts and "fixtures" in parts:
            continue

        errors.append(
            f"go: {rel_path} - nested go.mod found. "
            "All Go code should use the root go.mod."
        )


def check_rust(root: Path, errors: list[str]) -> None:
    """Check Rust monorepo rules."""
    root_cargo = root / "Cargo.toml"
    if not root_cargo.exists():
        return  # No Rust in this project

    try:
        cargo = parse_toml(root_cargo)
    except Exception as e:
        errors.append(f"rust: Failed to parse {root_cargo}: {e}")
        return

    workspace = cargo.get("workspace", {})
    if not workspace:
        return  # Not a workspace

    workspace_deps = workspace.get("dependencies", {})
    if not workspace_deps:
        return  # No workspace dependencies defined

    members = workspace.get("members", [])

    # Check each workspace member
    for member_pattern in members:
        if "*" in member_pattern:
            # Handle glob patterns like "cmd/*"
            base = member_pattern.replace("/*", "").replace("*", "")
            base_path = root / base
            if base_path.exists():
                for member_path in base_path.iterdir():
                    if member_path.is_dir():
                        check_rust_member(
                            root, member_path, workspace_deps, errors
                        )
        else:
            member_path = root / member_pattern
            if member_path.exists():
                check_rust_member(root, member_path, workspace_deps, errors)


def check_rust_member(
    root: Path,
    member_path: Path,
    workspace_deps: dict,
    errors: list[str],
) -> None:
    """Check a single Rust workspace member."""
    cargo_toml = member_path / "Cargo.toml"
    if not cargo_toml.exists():
        return

    try:
        cargo = parse_toml(cargo_toml)
    except Exception as e:
        errors.append(f"rust: Failed to parse {cargo_toml}: {e}")
        return

    rel_path = cargo_toml.relative_to(root)

    # Check [dependencies]
    check_rust_deps_section(
        rel_path, cargo.get("dependencies", {}), workspace_deps, "dependencies", errors
    )

    # Check [dev-dependencies]
    check_rust_deps_section(
        rel_path, cargo.get("dev-dependencies", {}), workspace_deps, "dev-dependencies", errors
    )

    # Check [build-dependencies]
    check_rust_deps_section(
        rel_path, cargo.get("build-dependencies", {}), workspace_deps, "build-dependencies", errors
    )


def check_rust_deps_section(
    cargo_path: Path,
    deps: dict,
    workspace_deps: dict,
    section: str,
    errors: list[str],
) -> None:
    """Check a dependencies section for workspace rule violations."""
    for dep_name, dep_spec in deps.items():
        # Normalize dep name (Cargo uses - but allows _)
        normalized_name = dep_name.replace("_", "-")
        alt_name = dep_name.replace("-", "_")

        # Check if this dep exists in workspace.dependencies
        ws_dep = workspace_deps.get(dep_name) or workspace_deps.get(normalized_name) or workspace_deps.get(alt_name)

        if ws_dep is not None:
            # This dep should use workspace = true
            if isinstance(dep_spec, str):
                # Direct version string like: dep = "1.0"
                errors.append(
                    f"rust: {cargo_path} [{section}] {dep_name} = \"{dep_spec}\" - "
                    f"should use '{dep_name}.workspace = true' (defined in workspace.dependencies)"
                )
            elif isinstance(dep_spec, dict):
                if not dep_spec.get("workspace"):
                    # Has attributes but not workspace = true
                    if "version" in dep_spec:
                        errors.append(
                            f"rust: {cargo_path} [{section}] {dep_name} has version = \"{dep_spec['version']}\" - "
                            f"should use '{dep_name}.workspace = true' (defined in workspace.dependencies)"
                        )
                    elif "path" in dep_spec and dep_name not in workspace_deps:
                        # Path deps that aren't in workspace are OK
                        pass
                    elif not dep_spec.get("workspace"):
                        errors.append(
                            f"rust: {cargo_path} [{section}] {dep_name} - "
                            f"should use '{dep_name}.workspace = true' (defined in workspace.dependencies)"
                        )


def check_python(root: Path, errors: list[str]) -> None:
    """Check Python monorepo rules."""
    root_pyproject = root / "pyproject.toml"
    if not root_pyproject.exists():
        return  # No Python in this project

    # Find all pyproject.toml files
    for pyproject in root.rglob("pyproject.toml"):
        if pyproject == root_pyproject:
            continue

        # Skip common non-source directories
        rel_path = pyproject.relative_to(root)
        parts = rel_path.parts
        if any(p in ("vendor", "node_modules", "buck-out", ".git", "cells", ".venv", "venv", "__pycache__") for p in parts):
            continue

        # Skip e2e fixtures
        if "e2e" in parts and "fixtures" in parts:
            continue

        # Check if this pyproject.toml has its own dependencies
        try:
            config = parse_toml(pyproject)
            project = config.get("project", {})
            deps = project.get("dependencies", [])
            optional_deps = project.get("optional-dependencies", {})

            if deps or optional_deps:
                errors.append(
                    f"python: {rel_path} has its own dependencies. "
                    "All Python deps should be in root pyproject.toml."
                )
        except Exception:
            pass  # Ignore parse errors for nested pyproject files


def check_javascript(root: Path, errors: list[str]) -> None:
    """Check JavaScript/TypeScript monorepo rules."""
    root_package = root / "package.json"
    if not root_package.exists():
        return  # No JS/TS in this project

    # Find all package.json files
    for package_json in root.rglob("package.json"):
        if package_json == root_package:
            continue

        # Skip common non-source directories
        rel_path = package_json.relative_to(root)
        parts = rel_path.parts
        if any(p in ("node_modules", "buck-out", ".git", "cells") for p in parts):
            continue

        # Skip e2e fixtures
        if "e2e" in parts and "fixtures" in parts:
            continue

        # Check if this package.json has direct dependencies (not workspace:)
        try:
            config = parse_json(package_json)
            deps = config.get("dependencies", {})
            dev_deps = config.get("devDependencies", {})

            direct_deps = []
            for name, version in {**deps, **dev_deps}.items():
                if isinstance(version, str) and not version.startswith("workspace:"):
                    direct_deps.append(f"{name}@{version}")

            if direct_deps:
                errors.append(
                    f"javascript: {rel_path} has direct dependencies: {', '.join(direct_deps[:3])}{'...' if len(direct_deps) > 3 else ''}. "
                    "Use 'workspace:*' protocol or move deps to root package.json."
                )
        except Exception:
            pass  # Ignore parse errors


def main() -> int:
    """Main entry point."""
    root = find_project_root()
    errors: list[str] = []

    print(f"Checking monorepo dependency rules in {root}...")
    print()

    # Run all checks
    check_go(root, errors)
    check_rust(root, errors)
    check_python(root, errors)
    check_javascript(root, errors)

    # Report results
    if errors:
        print(f"Found {len(errors)} monorepo dependency rule violation(s):\n")
        for error in errors:
            print(f"  - {error}")
        print()
        print("Fix: Move all dependencies to the root-level config file")
        print("     and use workspace references in sub-projects.")
        return 1

    print("All monorepo dependency rules are followed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
