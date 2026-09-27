"""Rules for the repository's foundry.toml.

The repository has at most one foundry.toml, at its root, so native forge
works from any directory and covers every Solidity package under src/. Two
rules keep it that way:

1. No foundry.toml other than the root one (a repository with none passes).
   A nested file becomes forge's root for its subtree and breaks the root
   workflow there.
2. No solc or solc_version key. The compiler comes from the toolchain,
   through FOUNDRY_SOLC in the dev shell. A pinned version here would go
   stale silently on a toolchain bump, since this check only runs when a
   foundry.toml changes.
"""

import tomllib
from pathlib import Path

# Directories whose foundry.toml files are not the repository's own:
# dependency checkouts and build output. Hidden directories (.turnkey,
# .devenv, .git, ...) are skipped too.
SKIPPED_DIRS = frozenset(("node_modules", "buck-out", "target"))

# Keys that pin the compiler.
COMPILER_KEYS = ("solc", "solc_version")


def find_foundry_configs(root: Path) -> list[Path]:
    """Find every foundry.toml under root, outside the skipped directories."""
    configs = []
    for path in root.rglob("foundry.toml"):
        parts = path.relative_to(root).parts
        if any(p in SKIPPED_DIRS or p.startswith(".") for p in parts[:-1]):
            continue
        configs.append(path)
    return sorted(configs)


def compiler_key_locations(config: dict) -> list[tuple[str, str]]:
    """Return (table, key) for every compiler-pinning key in a config."""
    found = [("top level", key) for key in COMPILER_KEYS if key in config]
    profiles = config.get("profile", {})
    if isinstance(profiles, dict):
        for name, profile in profiles.items():
            if isinstance(profile, dict):
                found += [
                    (f"[profile.{name}]", key)
                    for key in COMPILER_KEYS
                    if key in profile
                ]
    return found


def check_foundry_configs(root: Path) -> list[str]:
    """Check every foundry.toml under root and return the errors found."""
    errors: list[str] = []
    for path in find_foundry_configs(root):
        rel = path.relative_to(root).as_posix()
        if path.parent != root:
            errors.append(
                f"{rel}: only the root foundry.toml is allowed; move its "
                "settings to the root foundry.toml and delete this file"
            )
        try:
            with open(path, "rb") as f:
                config = tomllib.load(f)
        except (tomllib.TOMLDecodeError, OSError) as e:
            errors.append(f"{rel}: failed to parse: {e}")
            continue
        for table, key in compiler_key_locations(config):
            errors.append(
                f"{rel}: {table} sets {key!r}; the compiler comes from the "
                "toolchain (FOUNDRY_SOLC in the dev shell), remove the key"
            )
    return errors
