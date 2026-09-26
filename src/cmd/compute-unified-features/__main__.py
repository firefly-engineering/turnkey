#!/usr/bin/env python3
"""Compute unified features for all Rust crates in a vendor directory."""

import argparse
import json
from pathlib import Path

from turnkey.cargo import compute_unified_features, load_overrides, load_requested


def main():
    parser = argparse.ArgumentParser(
        prog="compute-unified-features",
        description="Compute unified features for all Rust crates in a vendor directory.",
    )
    parser.add_argument("vendor_dir", type=Path)
    parser.add_argument(
        "overrides_file", type=Path, nargs="?", help="rust-features.toml"
    )
    parser.add_argument(
        "--deps-file",
        type=Path,
        help="rust-deps.toml, whose [[requested]] entries are the workspace "
        "members' dependency specs to resolve features from",
    )
    args = parser.parse_args()

    overrides = load_overrides(args.overrides_file)
    requested = load_requested(args.deps_file)
    unified = compute_unified_features(args.vendor_dir, overrides, requested)

    # Output as JSON
    print(json.dumps(unified, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
