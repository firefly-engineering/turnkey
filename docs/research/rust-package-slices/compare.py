"""Compare rust-deps.toml's package slices with a built rustdeps cell.

For each crate, on each platform that builds it, the slice's features and
dependencies are compared with what the cell's rules.star gives that
platform (its select() branches resolved). Which platforms build a crate
comes from `cargo tree` dumps, one per platform:

    cargo tree --locked --workspace -e normal,dev --target <triple> \
        --prefix depth --format '{p}|{f}' > <dir>/<platform>.tree

Usage: compare.py <rust-deps.toml> <cell> <dir>

rules.star leaves out the `default` feature, so the comparison does too.
Labels that aren't another vendored crate (a fixup's native library) are
not dependencies.
"""

import ast
import sys
import tomllib
from pathlib import Path


def built_on(tree_dir, platforms):
    """{name@version: the platforms whose cargo tree has it}"""
    out = {}
    for p in platforms:
        for line in Path(tree_dir, f"{p}.tree").read_text().splitlines():
            if not line:
                continue
            package = line.lstrip("0123456789").split("|", 1)[0].split(" ")
            out.setdefault(f"{package[0]}@{package[1][1:]}", set()).add(p)
    return out


def applies(key, platforms):
    """The platforms a select() key covers: config//os:<os>,
    config//cpu:<cpu> or <settings>:<os>-<cpu>"""
    package, _, name = key.rpartition(":")
    if package == "config//os":
        return {p for p in platforms if p.split("-")[0] == name}
    if package == "config//cpu":
        return {p for p in platforms if p.split("-")[1] == name}
    return {name}


def per_platform(expr, merge, platforms):
    """{platform: value} of a list or dict expression, with + select({...})"""
    if isinstance(expr, ast.BinOp) and isinstance(expr.op, ast.Add):
        left = per_platform(expr.left, merge, platforms)
        right = per_platform(expr.right, merge, platforms)
        return {p: merge(left[p], right[p]) for p in platforms}
    if isinstance(expr, ast.Call) and expr.func.id == "select":
        out = {p: merge(None, None) for p in platforms}
        for key, value in ast.literal_eval(expr.args[0]).items():
            for p in applies(key, platforms):
                out[p] = merge(out[p], value)
        return out
    value = ast.literal_eval(expr)
    return {p: merge(None, value) for p in platforms}


def union(a, b):
    return sorted(set(a or []) | set(b or []))


def overlay(a, b):
    return {**(a or {}), **(b or {})}


def library(rules):
    """The keyword arguments of the rules.star's rust_library"""
    for node in ast.walk(ast.parse(rules)):
        if isinstance(node, ast.Call) and getattr(node.func, "id", "") == "rust_library":
            return {kw.arg: kw.value for kw in node.keywords}
    return None


def vendored(label):
    """The name@version of a rustdeps//vendor/<name@version>:<target> label"""
    return label.split("//vendor/")[1].split(":")[0] if "//vendor/" in label else None


def on(entry, platform):
    return platform in entry.get("platforms", [platform])


def main(deps_file, cell, tree_dir):
    doc = tomllib.loads(Path(deps_file).read_text())
    platforms = doc["platforms"]
    where = built_on(tree_dir, platforms)
    differing = 0
    for key, crate in sorted(doc["deps"].items()):
        star = Path(cell, "vendor", key, "rules.star")
        lib = library(star.read_text()) if star.exists() else None
        if lib is None or key not in where:
            continue

        def attr(name, merge, empty):
            if name not in lib:
                return {p: empty for p in platforms}
            return per_platform(lib[name], merge, platforms)

        old_features = attr("features", union, [])
        old_deps = attr("deps", union, [])
        old_named = attr("named_deps", overlay, {})
        report = []
        for p in sorted(where[key]):
            old = set(old_features[p]) - {"default"}
            new = {f["name"] for f in crate.get("features", []) if on(f, p)} - {"default"}
            if old != new:
                report.append(f"  {p} features: only today {sorted(old - new)}, only slice {sorted(new - old)}")
            old = {(vendored(d), None) for d in old_deps[p] if vendored(d)}
            old |= {(vendored(t), n) for n, t in old_named[p].items() if vendored(t)}
            new = {(d["package"], d.get("rename")) for d in crate.get("dependencies", []) if on(d, p)}
            if old != new:
                report.append(
                    f"  {p} deps: only today {sorted(old - new, key=str)}, only slice {sorted(new - old, key=str)}"
                )
        if report:
            differing += 1
            print(key)
            print("\n".join(report))
    print(f"{differing} of {len(doc['deps'])} crates differ")


if __name__ == "__main__":
    main(*sys.argv[1:])
