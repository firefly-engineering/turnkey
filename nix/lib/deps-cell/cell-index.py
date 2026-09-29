"""Write a deps cell's cell index (ADR 0004) from its Nix-built spec.

The spec names each package's store path and the file listing its target
names (one per line, written by the package's own generator); the index
carries the names themselves, so the materializer never reads Starlark.

Usage: cell-index.py <spec_json>   (the index is printed)
"""

import json
import sys
from pathlib import Path


def main() -> None:
    spec = json.loads(Path(sys.argv[1]).read_text())
    spec["packages"] = {
        path: {
            "store": package["store"],
            "targets": sorted(Path(package["targets"]).read_text().split()),
        }
        for path, package in spec["packages"].items()
    }
    print(json.dumps(spec, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
