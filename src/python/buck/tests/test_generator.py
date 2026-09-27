"""
Tests for resolving a vendored crate's dependencies to rustdeps targets.

Run with: tk test //src/python/buck:test_generator
"""

import unittest

from turnkey.buck.generator import (
    CrateFixup,
    PlatformRustcFlags,
    generate_buck_file,
    get_dependencies,
    get_fixup_env,
    get_fixup_rustc_flags,
)
from turnkey.cfg import Platforms

# turnkey's default platforms
PLATFORMS = Platforms.from_json(
    '{"settings": "toolchains//conditions", "platforms": ['
    '{"os": "linux", "cpu": "x86_64"}, {"os": "linux", "cpu": "arm64"}, '
    '{"os": "macos", "cpu": "x86_64"}, {"os": "macos", "cpu": "arm64"}]}'
)

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
        deps, named = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std"], PLATFORMS)
        self.assertEqual(deps.common, ["rustdeps//vendor/futures-core@0.3.31:futures-core"])
        self.assertEqual(named.common, {})

    def test_active_dep_with_no_matching_version_is_not_resolved(self):
        # compat is on, but futures 0.1 is not vendored: resolving futures_01
        # to futures 0.3 by name is the bug.
        _, named = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std", "compat"], PLATFORMS)
        self.assertEqual(named.common, {})

    def test_active_renamed_dep_resolves_by_version(self):
        available = AVAILABLE | {"futures@0.1.31"}
        _, named = get_dependencies(FUTURES_UTIL, available, ["std", "compat"], PLATFORMS)
        self.assertEqual(named.common, {"futures_01": "rustdeps//vendor/futures@0.1.31:futures"})

    def test_optional_dep_enabled_by_implicit_feature(self):
        deps, _ = get_dependencies(FUTURES_UTIL, AVAILABLE, ["std", "io", "memchr"], PLATFORMS)
        self.assertIn("rustdeps//vendor/memchr@2.7.4:memchr", deps.common)

    def test_picks_the_version_the_requirement_names(self):
        cargo = {"dependencies": {"getrandom": "0.2"}}
        available = {"getrandom@0.2.17", "getrandom@0.3.4", "getrandom"}
        deps, _ = get_dependencies(cargo, available, [], PLATFORMS)
        self.assertEqual(deps.common, ["rustdeps//vendor/getrandom@0.2.17:getrandom"])

    def test_unversioned_only_crate_resolves_by_name(self):
        cargo = {"dependencies": {"quote": "1"}}
        deps, _ = get_dependencies(cargo, {"quote"}, [], PLATFORMS)
        self.assertEqual(deps.common, ["rustdeps//vendor/quote:quote"])

    def test_target_specific_optional_dep_follows_features(self):
        cargo = {
            "features": {"fs": ["dep:libc"]},
            "target": {"cfg(unix)": {"dependencies": {"libc": {"version": "0.2", "optional": True}}}},
        }
        available = {"libc@0.2.170", "libc"}
        off, _ = get_dependencies(cargo, available, [], PLATFORMS)
        on, _ = get_dependencies(cargo, available, ["fs"], PLATFORMS)
        self.assertTrue(off.is_empty())
        self.assertFalse(on.is_empty())


    def test_linux_only_dep_is_keyed_on_the_os(self):
        cargo = {
            "dependencies": {"libc": "0.2"},
            "target": {'cfg(target_os = "linux")': {"dependencies": {"inotify": "0.11"}}},
        }
        deps, _ = get_dependencies(cargo, {"libc", "inotify"}, [], PLATFORMS)
        self.assertEqual(deps.common, ["rustdeps//vendor/libc:libc"])
        self.assertEqual(
            deps.by_platform,
            {"config//os:linux": ["rustdeps//vendor/inotify:inotify"], "config//os:macos": []},
        )

    def test_cpu_difference_within_an_os_uses_the_combined_key(self):
        cargo = {"target": {"aarch64-apple-darwin": {"dependencies": {"objc": "0.2"}}}}
        deps, _ = get_dependencies(cargo, {"objc"}, [], PLATFORMS)
        self.assertEqual(deps.common, [])
        self.assertEqual(
            deps.by_platform,
            {
                "toolchains//conditions:linux-arm64": [],
                "toolchains//conditions:linux-x86_64": [],
                "toolchains//conditions:macos-arm64": ["rustdeps//vendor/objc:objc"],
                "toolchains//conditions:macos-x86_64": [],
            },
        )

    def test_a_dep_every_platform_gets_is_common(self):
        cargo = {"target": {"cfg(unix)": {"dependencies": {"libc": "0.2"}}}}
        deps, _ = get_dependencies(cargo, {"libc"}, [], PLATFORMS)
        self.assertEqual(deps.common, ["rustdeps//vendor/libc:libc"])
        self.assertEqual(deps.by_platform, {})

    def test_a_dep_no_platform_gets_is_dropped(self):
        cargo = {"target": {"cfg(windows)": {"dependencies": {"winapi": "0.3"}}}}
        deps, _ = get_dependencies(cargo, {"winapi"}, [], PLATFORMS)
        self.assertTrue(deps.is_empty())


class TestGenerateBuckFile(unittest.TestCase):
    def test_select_has_a_branch_per_os_and_no_default(self):
        cargo = {"target": {'cfg(target_os = "linux")': {"dependencies": {"inotify": "0.11"}}}}
        deps, named = get_dependencies(cargo, {"inotify"}, [], PLATFORMS)
        content = generate_buck_file(
            "watch", "2021", None, deps, named, False, [], {}, PlatformRustcFlags(),
        )
        self.assertIn('"config//os:linux": [', content)
        self.assertIn('"config//os:macos": [],', content)
        self.assertNotIn("DEFAULT", content)


class TestFixupRustcFlags(unittest.TestCase):
    def test_per_os_flags_keep_their_order(self):
        fixup = CrateFixup.from_dict(
            {"rustcFlags": {"common": ["--cfg", "rustix_std"], "os": {"linux": ["--cfg", "linux_raw"]}, "cpu": {}}}
        )
        flags = get_fixup_rustc_flags(fixup, PLATFORMS)
        self.assertEqual(flags.common, ["--cfg", "rustix_std"])
        self.assertEqual(
            flags.by_platform,
            {"config//os:linux": ["--cfg", "linux_raw"], "config//os:macos": []},
        )

    def test_per_cpu_flags_are_keyed_on_the_cpu(self):
        fixup = CrateFixup.from_dict({"rustcFlags": {"common": [], "os": {}, "cpu": {"x86_64": ["--cfg", "x"]}}})
        flags = get_fixup_rustc_flags(fixup, PLATFORMS)
        self.assertEqual(flags.by_platform, {"config//cpu:arm64": [], "config//cpu:x86_64": ["--cfg", "x"]})

    def test_flags_every_platform_gets_are_common(self):
        fixup = CrateFixup.from_dict(
            {"rustcFlags": {"common": ["--cfg", "a"], "os": {"linux": ["--cfg", "b"], "macos": ["--cfg", "b"]}, "cpu": {}}}
        )
        flags = get_fixup_rustc_flags(fixup, PLATFORMS)
        self.assertEqual(flags.common, ["--cfg", "a", "--cfg", "b"])
        self.assertEqual(flags.by_platform, {})

    def test_no_fixup_means_no_flags(self):
        self.assertTrue(get_fixup_rustc_flags(CrateFixup(), PLATFORMS).is_empty())


class TestFixupEnv(unittest.TestCase):
    def test_env_differing_by_os_is_a_select(self):
        fixup = CrateFixup.from_dict({"env": {"common": {"A": "1"}, "os": {"macos": {"B": "2"}}, "cpu": {}}})
        common, by_platform = get_fixup_env(fixup, PLATFORMS)
        self.assertEqual(common, {"A": "1"})
        self.assertEqual(by_platform, {"config//os:linux": {}, "config//os:macos": {"B": "2"}})
        content = generate_buck_file(
            "c", "2021", None, get_dependencies({}, set(), [], PLATFORMS)[0],
            get_dependencies({}, set(), [], PLATFORMS)[1], False, [], common, PlatformRustcFlags(),
            env_by_platform=by_platform,
        )
        self.assertIn('    env = {\n        "A": "1",\n    } |\n    select({', content)


class TestNativeLibraries(unittest.TestCase):
    def test_native_library_is_linked_on_the_host_only(self):
        deps, named = get_dependencies({}, set(), [], PLATFORMS)
        content = generate_buck_file(
            "ring", "2021", None, deps, named, False, [], {}, PlatformRustcFlags(common=["--cap-lints", "allow"]),
            native_libraries=[{"lib_name": "ring_core", "static_lib_path": "out_dir/libring_core.a"}],
            host_key="toolchains//conditions:macos-arm64",
        )
        self.assertIn('prebuilt_cxx_library(\n    name = "ring_core",', content)
        self.assertIn('    deps = select({\n        "toolchains//conditions:macos-arm64": [\n            ":ring_core",', content)
        self.assertIn('    ] +\n    select({\n        "toolchains//conditions:macos-arm64": [\n            "-Lnative=out_dir",', content)
        self.assertEqual(content.count("select({"), 2)

    def test_native_library_without_a_host_is_an_error(self):
        deps, named = get_dependencies({}, set(), [], PLATFORMS)
        with self.assertRaises(ValueError):
            generate_buck_file(
                "ring", "2021", None, deps, named, False, [], {}, PlatformRustcFlags(),
                native_libraries=[{"lib_name": "r", "static_lib_path": "out_dir/libr.a"}],
            )


if __name__ == "__main__":
    unittest.main()
