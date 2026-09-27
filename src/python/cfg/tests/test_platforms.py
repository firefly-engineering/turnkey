"""
Tests for keying values that differ between platforms, as rules sync does
(src/go/pkg/conditions).

Run with: buck2 test //src/python/cfg:test
"""

import unittest

from turnkey.cfg.platforms import Platform, Platforms

PLATFORMS = Platforms(
    platforms=(
        Platform("linux", "x86_64"),
        Platform("linux", "arm64"),
        Platform("macos", "x86_64"),
        Platform("macos", "arm64"),
    ),
    settings="toolchains//conditions",
)


class TestSplit(unittest.TestCase):
    def test_same_everywhere_is_common(self):
        common, branches = PLATFORMS.split(lambda p: ["a", "b"])
        self.assertEqual(common, ["a", "b"])
        self.assertEqual(branches, {})

    def test_os_difference_uses_os_keys(self):
        common, branches = PLATFORMS.split(lambda p: ["u"] + (["l"] if p.os == "linux" else []))
        self.assertEqual(common, ["u"])
        self.assertEqual(branches, {"config//os:linux": ["l"], "config//os:macos": []})

    def test_cpu_difference_within_an_os_uses_the_combined_key(self):
        common, branches = PLATFORMS.split(lambda p: ["x"] if p == Platform("linux", "arm64") else [])
        self.assertEqual(common, [])
        self.assertEqual(
            branches,
            {
                "toolchains//conditions:linux-arm64": ["x"],
                "toolchains//conditions:linux-x86_64": [],
                "toolchains//conditions:macos-arm64": [],
                "toolchains//conditions:macos-x86_64": [],
            },
        )

    def test_cpu_only_difference_uses_cpu_keys(self):
        _, branches = PLATFORMS.split(lambda p: ["x"] if p.cpu == "x86_64" else [])
        self.assertEqual(branches, {"config//cpu:arm64": [], "config//cpu:x86_64": ["x"]})

    def test_never_default(self):
        _, branches = PLATFORMS.split(lambda p: [p.name])
        self.assertNotIn("DEFAULT", branches)
        self.assertEqual(len(branches), 4)


class TestFromJson(unittest.TestCase):
    def test_reads_settings_and_platforms(self):
        platforms = Platforms.from_json(
            '{"settings": "toolchains//conditions", "platforms": [{"os": "linux", "cpu": "x86_64"}]}'
        )
        self.assertEqual(list(platforms), [Platform("linux", "x86_64")])
        self.assertEqual(platforms.settings, "toolchains//conditions")

    def test_no_platforms_is_an_error(self):
        with self.assertRaises(ValueError):
            Platforms.from_json('{"settings": "x", "platforms": []}')


if __name__ == "__main__":
    unittest.main()
