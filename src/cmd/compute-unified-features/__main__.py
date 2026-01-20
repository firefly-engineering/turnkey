#!/usr/bin/env python3
"""Compute unified features for all Rust crates in a vendor directory."""

import json
import sys
from pathlib import Path

try:
    from cargo import compute_unified_features, load_overrides
except ImportError:
    from python.cargo import compute_unified_features, load_overrides


def main():
    if len(sys.argv) < 2:
        print(
            "Usage: compute-unified-features <vendor_dir> [overrides_file]",
            file=sys.stderr,
        )
        sys.exit(1)

    vendor_dir = Path(sys.argv[1])
    overrides_file = Path(sys.argv[2]) if len(sys.argv) > 2 else None

    overrides = load_overrides(overrides_file)
    unified = compute_unified_features(vendor_dir, overrides)

    # Output as JSON
    print(json.dumps(unified, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
