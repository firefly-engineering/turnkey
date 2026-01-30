#!/usr/bin/env python3
"""Check that Jest/Vitest configs properly exclude buck-out directories.

This script verifies that JavaScript/TypeScript test configurations exclude
buck-out directories to prevent spurious test failures from build artifacts
being picked up by test discovery.

Checks:
1. Jest configs (jest.config.js, jest.config.ts, jest field in package.json)
   - testPathIgnorePatterns should include '/buck-out/' or '/\\.'
2. Vitest configs (vitest.config.js, vitest.config.ts, vitest.config.mts)
   - exclude should include '**/buck-out/**' or '**/.*/**'
"""

import json
import re
import sys
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


def check_jest_config_file(config_path: Path, errors: list[str]) -> bool:
    """Check a Jest config file for buck-out exclusions.

    Returns True if the config was found and checked, False otherwise.
    """
    if not config_path.exists():
        return False

    content = config_path.read_text()

    # Look for testPathIgnorePatterns
    # Match patterns like:
    #   testPathIgnorePatterns: ['/buck-out/']
    #   testPathIgnorePatterns: ["/buck-out/", "/\\."]
    #   "testPathIgnorePatterns": [...]

    # Check if testPathIgnorePatterns is present
    if "testPathIgnorePatterns" not in content:
        errors.append(
            f"{config_path}: Jest config missing testPathIgnorePatterns. "
            "Add testPathIgnorePatterns: ['/buck-out/', '/\\\\.'] to exclude build artifacts."
        )
        return True

    # Check if buck-out or dot-directory exclusion is present
    # Look for patterns that would exclude buck-out
    has_buck_out = bool(re.search(r"['\"/]buck-out['\"/]", content))
    has_dot_dirs = bool(re.search(r"['\"/]\\\\?\\.?['\"/]", content)) or bool(
        re.search(r"['\"/]\.\*['\"/]", content)
    )

    if not has_buck_out and not has_dot_dirs:
        errors.append(
            f"{config_path}: Jest testPathIgnorePatterns should include '/buck-out/' or '/\\\\.' "
            "to exclude build artifacts from test discovery."
        )

    return True


def check_vitest_config_file(config_path: Path, errors: list[str]) -> bool:
    """Check a Vitest config file for buck-out exclusions.

    Returns True if the config was found and checked, False otherwise.
    """
    if not config_path.exists():
        return False

    content = config_path.read_text()

    # Look for exclude configuration
    # Match patterns like:
    #   exclude: ['**/buck-out/**']
    #   exclude: ["**/buck-out/**", "**/.*/**"]

    # Check if exclude is present in test config
    if "exclude" not in content:
        errors.append(
            f"{config_path}: Vitest config missing exclude pattern. "
            "Add exclude: ['**/buck-out/**', '**/.*/**'] to exclude build artifacts."
        )
        return True

    # Check if buck-out or dot-directory exclusion is present
    has_buck_out = bool(re.search(r"['\"].*buck-out.*['\"]", content))
    has_dot_dirs = bool(re.search(r"['\"].*\.\*\*.*['\"]", content)) or bool(
        re.search(r"['\"].*\/\.\*\/.*['\"]", content)
    )

    if not has_buck_out and not has_dot_dirs:
        errors.append(
            f"{config_path}: Vitest exclude should include '**/buck-out/**' or '**/.*/**' "
            "to exclude build artifacts from test discovery."
        )

    return True


def check_package_json_jest(package_json: Path, errors: list[str]) -> bool:
    """Check package.json for Jest config with buck-out exclusions.

    Returns True if Jest config was found in package.json, False otherwise.
    """
    if not package_json.exists():
        return False

    try:
        with open(package_json, "r") as f:
            config = json.load(f)
    except (json.JSONDecodeError, OSError):
        return False

    jest_config = config.get("jest")
    if jest_config is None:
        return False

    # Check testPathIgnorePatterns
    ignore_patterns = jest_config.get("testPathIgnorePatterns", [])
    if not ignore_patterns:
        errors.append(
            f"{package_json}: Jest config in package.json missing testPathIgnorePatterns. "
            "Add \"testPathIgnorePatterns\": [\"/buck-out/\", \"/\\\\.\"] to exclude build artifacts."
        )
        return True

    # Check if buck-out or dot-directory exclusion is present
    has_buck_out = any("buck-out" in p for p in ignore_patterns)
    has_dot_dirs = any(re.search(r"\\\\?\\.|\.\*", p) for p in ignore_patterns)

    if not has_buck_out and not has_dot_dirs:
        errors.append(
            f"{package_json}: Jest testPathIgnorePatterns should include '/buck-out/' or '/\\\\.' "
            "to exclude build artifacts from test discovery."
        )

    return True


def has_js_tests(root: Path) -> bool:
    """Check if the project has JavaScript/TypeScript test files."""
    test_patterns = [
        "**/*.test.js",
        "**/*.test.ts",
        "**/*.test.jsx",
        "**/*.test.tsx",
        "**/*.spec.js",
        "**/*.spec.ts",
        "**/*.spec.jsx",
        "**/*.spec.tsx",
        "**/test/**/*.js",
        "**/test/**/*.ts",
        "**/tests/**/*.js",
        "**/tests/**/*.ts",
        "**/__tests__/**/*.js",
        "**/__tests__/**/*.ts",
    ]

    for pattern in test_patterns:
        for path in root.glob(pattern):
            # Skip node_modules, buck-out, and hidden directories
            parts = path.relative_to(root).parts
            if any(
                p in ("node_modules", "buck-out", ".git", ".devenv", ".turnkey")
                or p.startswith(".")
                for p in parts
            ):
                continue
            return True

    return False


def main() -> int:
    """Main entry point."""
    root = find_project_root()
    errors: list[str] = []

    print(f"Checking JS/TS test config for buck-out exclusions in {root}...")
    print()

    # Track whether we found any test framework config
    found_jest = False
    found_vitest = False

    # Check Jest configs
    jest_configs = [
        root / "jest.config.js",
        root / "jest.config.ts",
        root / "jest.config.mjs",
        root / "jest.config.cjs",
    ]

    for config in jest_configs:
        if check_jest_config_file(config, errors):
            found_jest = True
            break

    # Check package.json for Jest config
    if not found_jest:
        found_jest = check_package_json_jest(root / "package.json", errors)

    # Check Vitest configs
    vitest_configs = [
        root / "vitest.config.js",
        root / "vitest.config.ts",
        root / "vitest.config.mjs",
        root / "vitest.config.mts",
    ]

    for config in vitest_configs:
        if check_vitest_config_file(config, errors):
            found_vitest = True
            break

    # If no test framework config found, check if tests exist
    if not found_jest and not found_vitest:
        if has_js_tests(root):
            print("Warning: Found JS/TS test files but no Jest or Vitest config.")
            print("         If using a test framework, ensure it excludes buck-out directories.")
            print()
            # This is just a warning, not an error
        else:
            print("No Jest or Vitest configuration found (and no test files detected).")
            return 0

    # Report results
    if errors:
        print(f"Found {len(errors)} test config issue(s):\n")
        for error in errors:
            print(f"  - {error}")
        print()
        print("Fix: Add exclusion patterns to prevent buck-out artifacts from test discovery:")
        print()
        print("  Jest (jest.config.js or package.json):")
        print('    testPathIgnorePatterns: ["/buck-out/", "/\\\\."]')
        print()
        print("  Vitest (vitest.config.ts):")
        print('    test: { exclude: ["**/buck-out/**", "**/.*/**", "**/node_modules/**"] }')
        return 1

    print("All test configs properly exclude buck-out directories.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
