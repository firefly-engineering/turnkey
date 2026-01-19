#!/usr/bin/env python3
"""Generate rules.star file for a Rust crate."""

import json
import sys
from pathlib import Path

try:
    from cargo import (
        parse_cargo_toml,
        get_crate_name,
        get_edition,
        get_lib_path,
        is_proc_macro,
        get_default_features,
        get_cargo_env,
    )
    from buck import (
        get_dependencies,
        get_build_script_cfg_flags,
        get_native_library_info,
        generate_buck_file,
        filter_features_for_availability,
    )
except ImportError:
    from python.cargo import (
        parse_cargo_toml,
        get_crate_name,
        get_edition,
        get_lib_path,
        is_proc_macro,
        get_default_features,
        get_cargo_env,
    )
    from python.buck import (
        get_dependencies,
        get_build_script_cfg_flags,
        get_native_library_info,
        generate_buck_file,
        filter_features_for_availability,
    )


def main():
    if len(sys.argv) < 3:
        print(
            "Usage: gen-rust-buck <crate_dir> <available_crates_json> "
            "[fixup_crates_json] [unified_features_json] [rustc_flags_registry_json]",
            file=sys.stderr,
        )
        sys.exit(1)

    crate_dir = Path(sys.argv[1])
    available_crates = set(json.loads(sys.argv[2]))
    fixup_crates = set(json.loads(sys.argv[3])) if len(sys.argv) > 3 else set()
    unified_features = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
    rustc_flags_registry = json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}

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
    deps, named_deps = get_dependencies(cargo, available_crates)
    proc_macro = is_proc_macro(cargo)
    env = get_cargo_env(cargo, crate_name)
    rustc_flags = get_build_script_cfg_flags(crate_name, version, rustc_flags_registry)

    # Get native library info for crates with pre-built native code
    native_lib_info = get_native_library_info(crate_name, version)

    # Use unified features if available, otherwise fall back to default features
    if crate_name in unified_features:
        features = unified_features[crate_name]
        # Still need to filter for availability (unified features may include
        # features that enable deps we don't have)
        features = filter_features_for_availability(features, cargo, available_crates)
    else:
        features = get_default_features(cargo, available_crates)

    # Add OUT_DIR for crates that have build script fixups
    if crate_name in fixup_crates:
        env["OUT_DIR"] = "out_dir"

    buck_content = generate_buck_file(
        crate_name,
        edition,
        crate_root,
        deps,
        named_deps,
        proc_macro,
        features,
        env,
        rustc_flags,
        native_lib_info,
    )
    print(buck_content)


if __name__ == "__main__":
    main()
