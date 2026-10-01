"""Write a deps cell's cell index (ADR 0004) from its Nix-built spec.

The spec names each package's store path and the file listing its target
names (one per line, written by the package's own generator); the index
carries the names themselves, so the materializer never reads Starlark.

A spec with modules (the Go cell, ADR 0008) names, for each module path, a
store path holding several Go packages: its targets file lists them as
"<subdir> <target>", its imports file the import paths they reference. Each
Go package becomes a package at vendor/<import path>, with its subdir; one
two modules offer goes to the longer module path. Each referenced import
path that no package provides and a go.work member owns (members: module
path -> directory) becomes a forwarding alias package, to the member's
package in the root cell.

Usage: cell-index.py <spec_json>   (the index is printed)
"""

import json
import sys
from pathlib import Path


def owner(import_path: str, module_paths) -> str | None:
    """The module path that owns import_path: its longest prefix, on a /
    boundary"""
    owners = [
        path
        for path in module_paths
        if import_path == path or import_path.startswith(path + "/")
    ]
    return max(owners, key=len, default=None)


def root_label(root_cell: str, member_dir: str, import_path: str, rest: str) -> str:
    """The label of a member's package: its directory in the project, and
    its import path's last component as the target, as buckgen names every
    Go package's"""
    parts = [part for part in member_dir.split("/") + rest.split("/") if part not in ("", ".")]
    return f"{root_cell}//{'/'.join(parts)}:{import_path.rsplit('/', 1)[-1]}"


def module_packages(spec: dict) -> tuple[dict, dict]:
    """The Go packages of the spec's modules, and the forwarding alias
    packages they need"""
    modules = spec["modules"]
    members = spec.get("members", {})

    packages = {}
    offered_by = {}
    referenced = set()
    for module_path, module in modules.items():
        for line in Path(module["targets"]).read_text().splitlines():
            if not line.strip():
                continue
            subdir, target = line.split()
            import_path = module_path if subdir == "." else f"{module_path}/{subdir}"
            # An import path two modules offer goes to the longer module path
            if import_path in offered_by and len(offered_by[import_path]) >= len(module_path):
                continue
            offered_by[import_path] = module_path
            package = {"store": module["store"], "targets": [target]}
            if subdir != ".":
                package["subdir"] = subdir
            packages[f"vendor/{import_path}"] = package
        referenced.update(Path(module["imports"]).read_text().split())

    forwards = {}
    for import_path in sorted(referenced):
        if f"vendor/{import_path}" in packages:
            continue
        module_path = owner(import_path, list(members) + list(modules))
        if module_path not in members:
            continue
        rest = import_path[len(module_path) + 1 :]
        forwards[f"vendor/{import_path}"] = root_label(
            spec["root_cell"], members[module_path], import_path, rest
        )
    return packages, forwards


def main() -> None:
    spec = json.loads(Path(sys.argv[1]).read_text())
    spec["packages"] = {
        path: {
            "store": package["store"],
            "targets": sorted(Path(package["targets"]).read_text().split()),
        }
        for path, package in spec["packages"].items()
    }
    if "modules" in spec:
        packages, forwards = module_packages(spec)
        spec["packages"].update(packages)
        if forwards:
            spec["forwards"] = forwards
        for key in ("modules", "members", "root_cell"):
            spec.pop(key, None)
    print(json.dumps(spec, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
