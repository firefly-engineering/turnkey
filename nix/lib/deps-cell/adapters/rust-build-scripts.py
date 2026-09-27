"""Fail a Rust deps cell when a crate's build script is unaccounted for.

Buck2 never runs a crate's build.rs, so every crate that has one needs a
fixup saying what stands in for it (nix/lib/fixups): the output it
generates, or that the build needs nothing from it. Whether a crate has a
build script is only known from its source, so it is checked here, when
the cell is built; the messages come from Nix, keyed "name@version", for
the crates no fixup accounts for.

Usage: rust-build-scripts.py <vendor_dir> <unaccounted_json>
"""

import json
import sys
import tomllib
from pathlib import Path


def has_build_script(crate_dir: Path) -> bool:
    """Whether Cargo would run a build script for the crate."""
    with open(crate_dir / "Cargo.toml", "rb") as f:
        build = tomllib.load(f).get("package", {}).get("build")
    if build is False:
        return False
    if isinstance(build, str):
        return (crate_dir / build).is_file()
    return (crate_dir / "build.rs").is_file()


def main() -> int:
    vendor = Path(sys.argv[1])
    unaccounted = json.loads(Path(sys.argv[2]).read_text())
    failures = [
        unaccounted[crate.name]
        for crate in sorted(vendor.iterdir())
        # The versioned directories; the unversioned names are symlinks
        if "@" in crate.name
        and crate.name in unaccounted
        and (crate / "Cargo.toml").is_file()
        and has_build_script(crate)
    ]
    for message in failures:
        print(f"error: {message}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
