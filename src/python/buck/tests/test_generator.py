"""
Tests for resolving a vendored crate's dependencies to rustdeps targets.

Run with: tk test //src/python/buck:test_generator
"""

import unittest

from turnkey.buck.generator import get_dependencies

# futures-util 0.3's manifest, trimmed to what matters: futures_01 is a
# renamed optional dependency on futures 0.1, enabled only by "compat".
FUTURES_UTIL = {
    "package": {"name": "futures-util", "version": "0.3.31"},
    "features": {
        "std": ["futures-core/std"],
        "compat": ["std", "dep:futures_01"],
        "io": ["std", "memchr"],
    },
    "dependencies": {
        "futures-core": {"version": "0.3.31", "default-features": False},
        "futures_01": {"version": "0.1.25", "package": "futures", "optional": True},
        "memchr": {"version": "2.2", "optional": True},
    },
}

# futures 0.3 depends on futures-util: vendoring it is what used to close the
# futures-util -> futures -> futures-util cycle.
AVAILABLE = {
    "futures-core@0.3.31",
    "futures-core",
    "futures@0.3.31",
    "futures",
    "memchr@2.7.4",
    "memchr",
}


class TestGetDependencies(unittest.TestCase):
    def test_inactive_renamed_optional_dep_is_dropped(self):
        deps, named = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std"])
        self.assertEqual(deps.common, ["rustdeps//vendor/futures-core@0.3.31:futures-core"])
        self.assertEqual(named.common, {})

    def test_active_dep_with_no_matching_version_is_not_resolved(self):
        # compat is on, but futures 0.1 is not vendored: resolving futures_01
        # to futures 0.3 by name is the bug.
        _, named = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std", "compat"])
        self.assertEqual(named.common, {})

    def test_active_renamed_dep_resolves_by_version(self):
        available = AVAILABLE | {"futures@0.1.31"}
        _, named = get_dependencies(FUTURES_UTIL, available, ["std", "compat"])
        self.assertEqual(named.common, {"futures_01": "rustdeps//vendor/futures@0.1.31:futures"})

    def test_optional_dep_enabled_by_implicit_feature(self):
        deps, _ = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std", "io", "memchr"])
        self.assertIn("rustdeps//vendor/memchr@2.7.4:memchr", deps.common)

    def test_picks_the_version_the_requirement_names(self):
        cargo = {"dependencies": {"getrandom": "0.2"}}
        available = {"getrandom@0.2.17", "getrandom@0.3.4", "getrandom"}
        deps, _ = get_dependencies(cargo, available, [])
        self.assertEqual(deps.common, ["rustdeps//vendor/getrandom@0.2.17:getrandom"])

    def test_unversioned_only_crate_resolves_by_name(self):
        cargo = {"dependencies": {"quote": "1"}}
        deps, _ = get_dependencies(cargo, {"quote"}, [])
        self.assertEqual(deps.common, ["rustdeps//vendor/quote:quote"])

    def test_target_specific_optional_dep_follows_features(self):
        cargo = {
            "features": {"fs": ["dep:libc"]},
            "target": {"cfg(unix)": {"dependencies": {"libc": {"version": "0.2", "optional": True}}}},
        }
        available = {"libc@0.2.170", "libc"}
        off, _ = get_dependencies(cargo, available, [])
        on, _ = get_dependencies(cargo, available, ["fs"])
        self.assertTrue(off.is_empty())
        self.assertFalse(on.is_empty())


if __name__ == "__main__":
    unittest.main()
