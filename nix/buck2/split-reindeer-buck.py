#!/usr/bin/env python3
"""
Split a reindeer-generated BUCK file into per-crate BUCK files.

Reindeer generates a single BUCK file with all crates. We need to split this
into individual BUCK files (vendor/<crate>/BUCK) for compatibility with
rustdeps//vendor/<crate>:<crate> target paths.

The script also rewrites paths from vendor/<crate>/src/... to src/...
since each BUCK file will be in the crate's directory.
"""

import re
import sys
from pathlib import Path
from collections import defaultdict


def parse_buck_file(content: str) -> dict[str, list[str]]:
    """
    Parse a BUCK file and extract rules by crate.

    Returns a dict mapping crate directory names to lists of rule strings.
    """
    crates = defaultdict(list)

    # Split by rule definitions
    # Rules start with: alias(, cargo.rust_library(, etc.
    rule_pattern = re.compile(r'^(alias|cargo\.rust_library|buildscript_run)\(', re.MULTILINE)

    # Find all rule starts
    matches = list(rule_pattern.finditer(content))

    for i, match in enumerate(matches):
        start = match.start()
        # End is either the next rule or end of file
        end = matches[i + 1].start() if i + 1 < len(matches) else len(content)

        rule_text = content[start:end].strip()

        # Determine which crate this rule belongs to
        # Look for patterns like vendor/crate-name/ in srcs or crate_root
        crate_match = re.search(r'vendor/([^/]+)/', rule_text)

        # Also check the rule name for versioned crates like "adler2-2.0.1"
        name_match = re.search(r'name\s*=\s*"([^"]+)"', rule_text)

        if crate_match:
            crate_dir = crate_match.group(1)
            crates[crate_dir].append(rule_text)
        elif name_match:
            # This is probably an alias - try to determine which crate from actual target
            actual_match = re.search(r'actual\s*=\s*":([^"]+)"', rule_text)
            if actual_match:
                # Extract crate from versioned name like "strsim-0.11.1"
                target_name = actual_match.group(1)
                # Remove version suffix to get directory name
                crate_dir = re.sub(r'-\d+\.\d+\.\d+.*$', '', target_name)
                crates[crate_dir].append(rule_text)

    return dict(crates)


def rewrite_paths(rule_text: str, crate_dir: str) -> str:
    """
    Rewrite paths in a rule from vendor/crate/... to relative paths.

    e.g., vendor/strsim/src/lib.rs -> src/lib.rs
    """
    # Replace vendor/crate_dir/ with empty string (makes paths relative)
    pattern = f'vendor/{re.escape(crate_dir)}/'
    return re.sub(pattern, '', rule_text)


def rewrite_deps(rule_text: str) -> str:
    """
    Rewrite dependency references from :crate-version to rustdeps//vendor/crate:crate.

    This is needed because dependencies cross crate boundaries.
    Handles both regular deps = [...] and named_deps = {...}
    """
    # Pattern for versioned dep references like ":strsim-0.11.1"
    # Include . in character class for version numbers
    def replace_dep(dep_match):
        dep_name = dep_match.group(1)
        # Extract crate name without version
        crate_name = re.sub(r'-\d+\.\d+\.\d+.*$', '', dep_name)
        return f'"rustdeps//vendor/{crate_name}:{crate_name}"'

    # Rewrite deps = [...] sections
    def replace_in_deps_section(match):
        deps_content = match.group(1)
        new_deps = re.sub(r'":([a-zA-Z0-9_.-]+)"', replace_dep, deps_content)
        return f'deps = [{new_deps}]'

    result = re.sub(r'deps = \[([^\]]*)\]', replace_in_deps_section, rule_text, flags=re.DOTALL)

    # Rewrite named_deps = {...} sections
    def replace_in_named_deps_section(match):
        deps_content = match.group(1)
        new_deps = re.sub(r'":([a-zA-Z0-9_.-]+)"', replace_dep, deps_content)
        return f'named_deps = {{{new_deps}}}'

    result = re.sub(r'named_deps = \{([^}]*)\}', replace_in_named_deps_section, result, flags=re.DOTALL)

    # Rewrite platform_deps = {...} if present
    def replace_in_platform_deps_section(match):
        deps_content = match.group(1)
        new_deps = re.sub(r'":([a-zA-Z0-9_.-]+)"', replace_dep, deps_content)
        return f'platform_deps = [{new_deps}]'

    result = re.sub(r'platform_deps = \[([^\]]*)\]', replace_in_platform_deps_section, result, flags=re.DOTALL)

    return result


def extract_crate_name(rule_text: str) -> str | None:
    """Extract the target name from a rule."""
    match = re.search(r'name\s*=\s*"([^"]+)"', rule_text)
    return match.group(1) if match else None


def main():
    if len(sys.argv) != 3:
        print("Usage: split-reindeer-buck.py <input_buck_file> <output_vendor_dir>", file=sys.stderr)
        sys.exit(1)

    input_file = Path(sys.argv[1])
    vendor_dir = Path(sys.argv[2])

    # Read the input BUCK file
    content = input_file.read_text()

    # Parse and group rules by crate
    crates = parse_buck_file(content)

    # Extract the load statement from the beginning
    load_match = re.search(r'^load\([^)]+\)\s*', content, re.MULTILINE)
    load_stmt = load_match.group(0) if load_match else ""

    # Write per-crate BUCK files
    for crate_dir, rules in crates.items():
        crate_path = vendor_dir / crate_dir
        if not crate_path.exists():
            continue

        buck_file = crate_path / "BUCK"

        # Build the BUCK file content
        lines = [
            "# Auto-generated from reindeer output by split-reindeer-buck.py",
            'load("@prelude//rust:cargo_buildscript.bzl", "buildscript_run")',
            'load("@prelude//rust:cargo_package.bzl", "cargo")',
            "",
        ]

        # Track which rules we've added (avoid duplicates)
        seen_names = set()

        for rule_text in rules:
            # Rewrite paths to be relative to this directory
            rule_text = rewrite_paths(rule_text, crate_dir)
            # Rewrite deps to use full cell paths
            rule_text = rewrite_deps(rule_text)

            # Get the name to check for duplicates
            name = extract_crate_name(rule_text)
            if name and name in seen_names:
                continue
            if name:
                seen_names.add(name)

            lines.append(rule_text)
            lines.append("")

        # Write the crate name alias if not already present
        # The main target should be accessible as just the crate name
        base_name = re.sub(r'-\d+\.\d+\.\d+.*$', '', crate_dir)
        if base_name not in seen_names:
            # Find the versioned target name
            for name in seen_names:
                if name.startswith(f"{base_name}-"):
                    lines.append(f'alias(name = "{base_name}", actual = ":{name}", visibility = ["PUBLIC"])')
                    lines.append("")
                    break

        buck_file.write_text("\n".join(lines))

    print(f"Split BUCK file into {len(crates)} per-crate files", file=sys.stderr)


if __name__ == "__main__":
    main()
