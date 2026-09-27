#!/usr/bin/env python3
"""Check the repository's foundry.toml: at most one, at the root, no solc pin.

See turnkey/check_foundry_config.py for the rules and why they hold.
"""

import sys
from pathlib import Path

from turnkey.check_foundry_config import check_foundry_configs


def find_project_root() -> Path:
    """Find the project root (directory with .git or .buckroot)."""
    root = Path.cwd()
    while root != root.parent:
        if (root / ".git").exists() or (root / ".buckroot").exists():
            return root
        root = root.parent
    return Path.cwd()


def main() -> int:
    root = find_project_root()
    errors = check_foundry_configs(root)
    if errors:
        print(f"foundry.toml errors ({len(errors)}):")
        for error in errors:
            print(f"  - {error}")
        return 1
    print("foundry.toml configuration is valid.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
