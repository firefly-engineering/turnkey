"""Feature unification for Rust crates.

This module implements Cargo-style feature unification over a vendor
directory. Starting from what the workspace members request (their
dependency specs, recorded in rust-deps.toml by rustdeps-gen), it walks the
dependency graph the way Cargo's resolver does:

- a dependency spec resolves to the vendored version its requirement
  matches, so each version of a crate unifies separately;
- a crate's default features are on only when some requirer asks for them
  (default-features is not false);
- an optional dependency counts as a requirer only when an enabled feature
  activates it, and "dep/feature" forwarding requests features on it.

The result maps each vendored crate ("name@version") to the features it is
built with.
"""

import tomllib
from collections import defaultdict
from collections.abc import Collection, Iterable
from dataclasses import dataclass, field
from pathlib import Path

from .semver import best_match
from .toml import (
    parse_cargo_toml,
    get_crate_name,
    get_version_req,
    normalize_crate_name,
    extract_dep_features,
    get_dep_package_name,
    is_optional,
)
try:
    from cfg import classify_target_platforms
except ImportError:
    from turnkey.cfg import classify_target_platforms


def parse_feature_forwarding(feature_item: str) -> tuple[str, str] | None:
    """
    Parse feature forwarding syntax: "dep/feature" or "dep?/feature"

    Returns (dep_name, feature_name) or None if not feature forwarding.
    """
    if "/" not in feature_item:
        return None

    # Handle optional dep syntax: "dep?/feature"
    if "?" in feature_item:
        dep_part, feature = feature_item.split("/", 1)
        dep_name = dep_part.rstrip("?")
    else:
        dep_name, feature = feature_item.split("/", 1)

    return (dep_name, feature)


def dependency_tables(cargo: dict) -> list[dict]:
    """A crate's [dependencies] and every [target.*.dependencies] table."""
    tables = [cargo.get("dependencies", {})]
    for target_config in cargo.get("target", {}).values():
        tables.append(target_config.get("dependencies", {}))
    return tables


@dataclass
class Activation:
    """What a set of requested features turns on in one crate.

    features: the enabled features, transitively expanded ("default" is
        expanded but not reported, and dep:/forwarding items are not features)
    optional_deps: the optional dependencies (by their key in the manifest)
        the features activate
    dep_features: features to request on dependencies (by manifest key),
        from "dep/feature" items and from "dep?/feature" items whose
        dependency is active
    """

    features: set[str] = field(default_factory=set)
    optional_deps: set[str] = field(default_factory=set)
    dep_features: dict[str, set[str]] = field(default_factory=dict)


def activate(
    cargo: dict, requested: Iterable[str], remove: Collection[str] = frozenset()
) -> Activation:
    """Expand `requested` features of a crate as Cargo does.

    Features in `remove` are treated as absent, so what only they would turn
    on stays off; removing "default" drops the crate's default set.
    """
    defs = cargo.get("features", {})
    optional: set[str] = set()
    required: set[str] = set()
    for table in dependency_tables(cargo):
        for key, spec in table.items():
            if is_optional(spec):
                optional.add(key)
            else:
                required.add(key)
    # An optional dependency named with "dep:" anywhere has no implicit
    # feature of its own name.
    named_with_dep = {
        item[len("dep:") :]
        for items in defs.values()
        for item in items
        if item.startswith("dep:")
    }

    result = Activation()
    enabled: set[str] = set()
    forwards: list[tuple[str, str, bool]] = []
    to_process = list(requested)
    while to_process:
        feature = to_process.pop()
        if feature in remove or feature in enabled:
            continue
        if feature.startswith("dep:"):
            result.optional_deps.add(feature[len("dep:") :])
            continue
        fwd = parse_feature_forwarding(feature)
        if fwd:
            dep, dep_feature = fwd
            weak = feature.split("/", 1)[0].endswith("?")
            forwards.append((dep, dep_feature, weak))
            if not weak and dep in optional:
                result.optional_deps.add(dep)
                # ...and turns on the feature of the dependency's name, if
                # there is one (explicit, or implicit)
                if dep in defs or dep not in named_with_dep:
                    to_process.append(dep)
            continue
        enabled.add(feature)
        if feature in defs:
            to_process.extend(defs[feature])
        elif feature in optional and feature not in named_with_dep:
            result.optional_deps.add(feature)

    active = required | result.optional_deps
    for dep, dep_feature, weak in forwards:
        if weak and dep not in active:
            continue
        result.dep_features.setdefault(dep, set()).add(dep_feature)

    enabled.discard("default")
    result.features = enabled
    return result


def load_vendored_crates(vendor_dir: Path) -> dict[str, dict]:
    """Parse every crate in a vendor dir, keyed by "name@version".

    Symlinks (the unversioned "name" aliases) are skipped.
    """
    crates: dict[str, dict] = {}
    for crate_dir in sorted(vendor_dir.iterdir()):
        if not crate_dir.is_dir() or crate_dir.is_symlink():
            continue
        cargo = parse_cargo_toml(crate_dir)
        if not cargo:
            continue
        dir_name, _, dir_version = crate_dir.name.partition("@")
        name = get_crate_name(cargo, dir_name)
        version = cargo.get("package", {}).get("version", dir_version)
        crates[f"{name}@{version}"] = cargo
    return crates


def supported_dependency_specs(cargo: dict) -> dict[str, list]:
    """A crate's normal dependency specs by manifest key.

    Covers [dependencies] and the target tables that apply on any supported
    platform: a crate is built with one feature set everywhere, so it must
    hold what each platform's dependents ask for (dev-dependencies don't
    affect library builds, and build scripts are not built from
    build-dependencies).
    """
    specs: dict[str, list] = defaultdict(list)
    for key, spec in cargo.get("dependencies", {}).items():
        specs[key].append(spec)
    for target_spec, target_config in cargo.get("target", {}).items():
        if not classify_target_platforms(target_spec):
            continue
        for key, spec in target_config.get("dependencies", {}).items():
            specs[key].append(spec)
    return specs


class _Unifier:
    """Worklist resolution of features from a set of requests."""

    def __init__(self, crates: dict[str, dict], overrides: dict):
        self.crates = crates
        # normalized name -> version -> "name@version"
        self.versions: dict[str, dict[str, str]] = defaultdict(dict)
        for key in crates:
            name, _, version = key.rpartition("@")
            self.versions[normalize_crate_name(name)][version] = key
        self.replaced: dict[str, list[str]] = {}
        self.added: dict[str, list[str]] = {}
        self.removed: dict[str, set[str]] = {}
        for key in crates:
            override = overrides.get(key.rpartition("@")[0])
            if isinstance(override, list):
                self.replaced[key] = override
            elif isinstance(override, dict):
                self.added[key] = override.get("add", [])
                self.removed[key] = set(override.get("remove", []))
        self.requested: dict[str, set[str]] = {}
        self.pending: list[str] = []

    def resolve(self, pkg_name: str, version_req: str | None) -> str | None:
        """The vendored "name@version" a dependency on pkg_name resolves to."""
        versions = self.versions.get(normalize_crate_name(pkg_name), {})
        match = best_match(version_req, versions)
        return versions[match] if match else None

    def request(self, key: str, features) -> None:
        """Ask for features on a crate; an empty request still reaches it."""
        features = set(features)
        requested = self.requested.get(key)
        if requested is not None and features <= requested:
            return
        self.requested[key] = (requested or set()) | features
        if key not in self.pending:
            self.pending.append(key)

    def activation(self, key: str, requested) -> Activation:
        features = self.replaced.get(key, set(requested) | set(self.added.get(key, [])))
        return activate(self.crates[key], features, self.removed.get(key, frozenset()))

    def run(self) -> None:
        while self.pending:
            key = self.pending.pop()
            activation = self.activation(key, self.requested[key])
            specs = supported_dependency_specs(self.crates[key])
            for dep_key, dep_specs in specs.items():
                forwarded = activation.dep_features.get(dep_key, set())
                for spec in dep_specs:
                    # A key can be optional in one table and required in
                    # another (per platform)
                    if is_optional(spec) and dep_key not in activation.optional_deps:
                        continue
                    pkg_name = get_dep_package_name(dep_key, spec)
                    target = self.resolve(pkg_name, get_version_req(spec))
                    if target:
                        self.request(target, [*extract_dep_features(spec), *forwarded])

    def features(self, key: str) -> list[str]:
        if key in self.replaced:
            return sorted(self.replaced[key])
        # A crate nothing reaches (a build-dependency, a dependency on an
        # unsupported platform) is not built from the graph; give it its
        # defaults.
        requested = self.requested.get(key, {"default"})
        return sorted(self.activation(key, requested).features)


def compute_unified_features(
    vendor_dir: Path, overrides: dict, requested: list[dict] | None = None
) -> dict[str, list[str]]:
    """
    Compute unified features for all crates.

    Args:
        vendor_dir: Path to vendor directory containing crate sources
        overrides: Manual feature overrides from rust-features.toml, by crate
            name: a list replaces the computed features, a dict's "add" is
            requested on top and its "remove" is never enabled
        requested: The workspace members' dependency specs ({"name",
            "version", "features", "default-features"}). None when unknown
            (a rust-deps.toml from before rustdeps-gen recorded them): every
            crate is then requested with its defaults.

    Returns:
        Dict mapping "name@version" to sorted lists of features
    """
    crates = load_vendored_crates(vendor_dir)
    unifier = _Unifier(crates, overrides)

    if requested is None:
        for key in crates:
            unifier.request(key, ["default"])
    else:
        for spec in requested:
            key = unifier.resolve(spec["name"], get_version_req(spec))
            if key:
                unifier.request(key, extract_dep_features(spec))
    for key in unifier.added:
        unifier.request(key, [])
    unifier.run()

    return {key: unifier.features(key) for key in crates}


def load_overrides(overrides_file: Path | None) -> dict:
    """Load feature overrides from rust-features.toml."""
    if overrides_file is None or not overrides_file.exists():
        return {}

    with open(overrides_file, "rb") as f:
        data = tomllib.load(f)

    return data.get("overrides", {})


def load_requested(deps_file: Path | None) -> list[dict] | None:
    """Load the workspace's dependency requests from rust-deps.toml.

    Returns None when the file predates rustdeps-gen recording them.
    """
    if deps_file is None or not deps_file.exists():
        return None

    with open(deps_file, "rb") as f:
        data = tomllib.load(f)

    return data.get("requested")
