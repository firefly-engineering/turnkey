#!/usr/bin/env python3
"""Check that all source files are covered by Buck2 targets in rules.star files.

This script ensures no source files are accidentally forgotten when adding
new code to the repository. It parses rules.star files to extract source
patterns (both glob patterns and explicit file lists) and compares them
against actual source files on disk.

Usage:
    python -m check-source-coverage [--scope=src/]

Configuration:
    The scope can be configured via the TURNKEY_SOURCE_SCOPE environment
    variable or the --scope flag. Default is "src/".
"""

import argparse
import fnmatch
import os
import re
import sys
from pathlib import Path


# Source file extensions we care about
SOURCE_EXTENSIONS = {
    ".go",
    ".rs",
    ".py",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".sol",
}

# Directories to always exclude
EXCLUDED_DIRS = {
    "__pycache__",
    ".git",
    "node_modules",
    "target",
    "buck-out",
    ".turnkey",
    ".devenv",
    "vendor",  # External dependencies
}

# Files to always exclude
EXCLUDED_FILES = {
    "__init__.py",  # Often not explicitly listed but implicitly included
}


def find_project_root() -> Path:
    """Find the project root (directory with .git or flake.nix)."""
    cwd = Path.cwd()
    root = cwd

    while root != root.parent:
        if (root / ".git").exists() or (root / "flake.nix").exists():
            return root
        root = root.parent

    return cwd


def parse_starlark_list(content: str, start_pos: int) -> tuple[list[str], int]:
    """Parse a Starlark list starting at [ and return items and end position."""
    items = []
    pos = start_pos
    depth = 0
    current_item = ""
    in_string = False
    string_char = None

    while pos < len(content):
        char = content[pos]

        if not in_string:
            if char in ('"', "'"):
                in_string = True
                string_char = char
            elif char == "[":
                depth += 1
                if depth == 1:
                    # Skip the opening bracket, don't add to current_item
                    pos += 1
                    continue
            elif char == "]":
                depth -= 1
                if depth == 0:
                    if current_item.strip():
                        items.append(current_item.strip().strip('"\''))
                    return items, pos + 1
            elif char == "," and depth == 1:
                if current_item.strip():
                    items.append(current_item.strip().strip('"\''))
                current_item = ""
                pos += 1
                continue
        else:
            if char == string_char and (pos == 0 or content[pos - 1] != "\\"):
                in_string = False
                string_char = None

        if depth >= 1:
            current_item += char

        pos += 1

    return items, pos


def extract_srcs_patterns(rules_star_path: Path) -> list[tuple[str, list[str]]]:
    """Extract source patterns from a rules.star file.

    Returns a list of (target_name, patterns) tuples where patterns
    can be glob patterns or explicit file paths.
    """
    content = rules_star_path.read_text()
    results = []

    # Find all srcs = ... patterns
    # This handles:
    #   srcs = glob(["src/**/*.rs"])
    #   srcs = ["file1.py", "file2.py"]
    #   srcs = glob(["*.go"])
    #   main = "main.py"  (python_binary, typescript, etc.)
    #   crate_root = "src/main.rs" (rust binaries)
    #   src = "file.txt" (export_file)

    # Pattern for srcs = glob([...])
    glob_pattern = re.compile(r'srcs\s*=\s*glob\s*\(\s*\[')
    # Pattern for srcs = [...]
    list_pattern = re.compile(r'srcs\s*=\s*\[')
    # Pattern for main = "..." or crate_root = "..." or src = "..."
    single_file_pattern = re.compile(r'(?:main|crate_root|src)\s*=\s*"([^"]+)"')
    # Pattern for name = "..."
    name_pattern = re.compile(r'name\s*=\s*"([^"]+)"')

    lines = content.split("\n")
    current_target = "unknown"

    for i, line in enumerate(lines):
        # Track current target name
        name_match = name_pattern.search(line)
        if name_match:
            current_target = name_match.group(1)

        # Check for main = "..." or crate_root = "..." or src = "..."
        single_file_match = single_file_pattern.search(line)
        if single_file_match:
            results.append((current_target, [single_file_match.group(1)]))

        # Check for srcs = glob([...])
        glob_match = glob_pattern.search(line)
        if glob_match:
            # Find the start of the list
            start = content.find("[", content.find("glob", sum(len(l) + 1 for l in lines[:i])))
            if start != -1:
                patterns, _ = parse_starlark_list(content, start)
                if patterns:
                    results.append((current_target, patterns))
            continue

        # Check for srcs = [...] (not glob)
        list_match = list_pattern.search(line)
        if list_match and "glob" not in line:
            start = line.find("[") + sum(len(l) + 1 for l in lines[:i])
            patterns, _ = parse_starlark_list(content, start)
            if patterns:
                results.append((current_target, patterns))

    return results


def expand_glob_pattern(base_dir: Path, pattern: str) -> set[Path]:
    """Expand a glob pattern relative to a base directory."""
    # Convert pattern to work with pathlib
    # Handle ** for recursive matching
    if "**" in pattern:
        # Use rglob for recursive patterns
        parts = pattern.split("**")
        if len(parts) == 2:
            prefix = parts[0].rstrip("/")
            suffix = parts[1].lstrip("/")
            search_dir = base_dir / prefix if prefix else base_dir
            if search_dir.exists():
                return {
                    p for p in search_dir.rglob(suffix)
                    if p.is_file()
                }
    else:
        # Simple glob
        return {p for p in base_dir.glob(pattern) if p.is_file()}

    return set()


def expand_patterns(rules_star_dir: Path, patterns: list[str]) -> set[Path]:
    """Expand patterns (globs or explicit files) relative to rules.star directory."""
    files = set()

    for pattern in patterns:
        if "*" in pattern:
            # It's a glob pattern
            files.update(expand_glob_pattern(rules_star_dir, pattern))
        else:
            # It's an explicit file path
            file_path = rules_star_dir / pattern
            if file_path.exists():
                files.add(file_path)

    return files


def find_all_source_files(scope_dir: Path) -> set[Path]:
    """Find all source files in the given scope directory."""
    files = set()

    for root, dirs, filenames in os.walk(scope_dir):
        # Filter out excluded directories
        dirs[:] = [d for d in dirs if d not in EXCLUDED_DIRS]

        root_path = Path(root)
        for filename in filenames:
            if filename in EXCLUDED_FILES:
                continue
            file_path = root_path / filename
            if file_path.suffix in SOURCE_EXTENSIONS:
                files.add(file_path)

    return files


def find_rules_star_files(scope_dir: Path) -> list[Path]:
    """Find all rules.star files in the scope directory."""
    rules_files = []

    for root, dirs, filenames in os.walk(scope_dir):
        # Filter out excluded directories
        dirs[:] = [d for d in dirs if d not in EXCLUDED_DIRS]

        if "rules.star" in filenames:
            rules_files.append(Path(root) / "rules.star")

    return rules_files


def main() -> int:
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description="Check that all source files are covered by Buck2 targets"
    )
    parser.add_argument(
        "--scope",
        default=os.environ.get("TURNKEY_SOURCE_SCOPE", "src/"),
        help="Directory scope to check (default: src/)",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Show verbose output including covered files",
    )
    args = parser.parse_args()

    project_root = find_project_root()
    scope_dir = project_root / args.scope

    if not scope_dir.exists():
        print(f"Error: Scope directory does not exist: {scope_dir}")
        return 1

    print(f"Checking source coverage in: {scope_dir}")

    # Find all source files
    all_source_files = find_all_source_files(scope_dir)
    print(f"Found {len(all_source_files)} source files")

    # Find all rules.star files and extract patterns
    rules_files = find_rules_star_files(scope_dir)
    print(f"Found {len(rules_files)} rules.star files")

    # Track covered files
    covered_files: set[Path] = set()
    coverage_map: dict[Path, list[str]] = {}  # file -> [targets that cover it]

    for rules_file in rules_files:
        rules_dir = rules_file.parent
        try:
            target_patterns = extract_srcs_patterns(rules_file)
            for target_name, patterns in target_patterns:
                target_files = expand_patterns(rules_dir, patterns)
                for f in target_files:
                    covered_files.add(f)
                    if f not in coverage_map:
                        coverage_map[f] = []
                    coverage_map[f].append(f"{rules_file.relative_to(project_root)}:{target_name}")
        except Exception as e:
            print(f"Warning: Failed to parse {rules_file}: {e}")

    # Find uncovered files
    uncovered_files = all_source_files - covered_files

    if args.verbose:
        print(f"\nCovered files: {len(covered_files)}")
        for f in sorted(covered_files):
            targets = coverage_map.get(f, [])
            print(f"  {f.relative_to(project_root)} -> {', '.join(targets)}")

    if uncovered_files:
        print(f"\nFound {len(uncovered_files)} uncovered source file(s):\n")
        for f in sorted(uncovered_files):
            rel_path = f.relative_to(project_root)
            print(f"  - {rel_path}")

        print("\nThese files are not included in any rules.star target.")
        print("Either add them to an existing target or create a new one.")
        print("\nExample fixes:")
        print("  1. Add to existing target: srcs = glob([\"src/**/*.rs\"])")
        print("  2. Add explicitly: srcs = [\"newfile.py\", ...]")
        return 1

    print(f"\nAll {len(all_source_files)} source files are covered by Buck2 targets")
    return 0


if __name__ == "__main__":
    sys.exit(main())
