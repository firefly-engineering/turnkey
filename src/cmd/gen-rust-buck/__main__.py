#!/usr/bin/env python3
"""Generate rules.star file for a Rust crate."""

import json
import sys
from pathlib import Path

from turnkey.cargo import (
    parse_cargo_toml,
    get_crate_name,
    get_edition,
    get_lib_path,
    is_proc_macro,
    get_default_features,
    get_cargo_env,
)
from turnkey.cfg import Platform, Platforms
from turnkey.buck import (
    CrateFixup,
    get_dependencies,
    get_fixup_env,
    get_fixup_rustc_flags,
    generate_buck_file,
    filter_features_for_availability,
)


def main():
    if len(sys.argv) != 7:
        print(
            "Usage: gen-rust-buck <crate_dir> <available_crates_json> "
            "<fixups_json_file> <unified_features_json> <platforms_json> <host_json>",
            file=sys.stderr,
        )
        sys.exit(1)

    crate_dir = Path(sys.argv[1])
    available_crates = set(json.loads(sys.argv[2]))
    # Each locked crate's fixup, keyed "name@version" (nix/lib/fixups/resolve.nix)
    fixups = json.loads(Path(sys.argv[3]).read_text())
    unified_features = json.loads(sys.argv[4])
    # The platforms the cell is built for (turnkey's buck2.platforms)
    platforms = Platforms.from_json(sys.argv[5])
    # The platform the cell is built on: what fixups built natively exists
    # only for it
    host_doc = json.loads(sys.argv[6])
    host_key = platforms.key(("os", "cpu"), Platform(host_doc["os"], host_doc["cpu"]))

    # Get crate name from directory (format: name@version or just name)
    dir_name = crate_dir.name
    if "@" in dir_name:
        fallback_name = dir_name.split("@")[0]
    else:
        fallback_name = dir_name

    cargo = parse_cargo_toml(crate_dir)
    crate_name = get_crate_name(cargo, fallback_name)
    version = cargo.get("package", {}).get("version", "0.0.0")
    edition = get_edition(cargo, crate_dir=crate_dir)
    crate_root = get_lib_path(cargo, crate_dir)
    proc_macro = is_proc_macro(cargo)
    env = get_cargo_env(cargo, crate_name)
    versioned_key = f"{crate_name}@{version}"
    fixup = CrateFixup.from_dict(fixups.get(versioned_key, {}))
    rustc_flags = get_fixup_rustc_flags(fixup, platforms)
    # Cap lints for vendored crates, as Cargo does for every non-local
    # dependency: a crate's own #![deny(...)] must not break the build when a
    # newer rustc adds lints.
    rustc_flags.common = ["--cap-lints", "allow"] + rustc_flags.common

    # Use unified features (keyed by name@version) if available, otherwise
    # fall back to default features
    unified = unified_features.get(versioned_key)
    if unified is not None:
        # Still need to filter for availability (unified features may include
        # features that enable deps we don't have)
        features = filter_features_for_availability(unified, cargo, available_crates)
    else:
        features = get_default_features(cargo, available_crates)

    # Optional deps are only included when these features activate them
    platform_deps, platform_named_deps = get_dependencies(
        cargo, available_crates, features, platforms
    )

    # The fixup's env, and OUT_DIR when its build script generated output
    fixup_env, env_by_platform = get_fixup_env(fixup, platforms)
    env.update(fixup_env)
    if fixup.out_dir:
        env["OUT_DIR"] = "out_dir"

    buck_content = generate_buck_file(
        crate_name,
        edition,
        crate_root,
        platform_deps,
        platform_named_deps,
        proc_macro,
        features,
        env,
        rustc_flags,
        native_libraries=fixup.native_libraries,
        host_key=host_key,
        env_by_platform=env_by_platform,
    )
    print(buck_content)


if __name__ == "__main__":
    main()
