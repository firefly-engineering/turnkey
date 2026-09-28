"""
Tests for the foundry.toml rules check-foundry-config enforces.

Run with: tk test //src/cmd/check-foundry-config:test
"""

import tempfile
import unittest
from pathlib import Path

from turnkey.check_foundry_config import check_foundry_configs

ROOT_CONFIG = """\
[profile.default]
src = "src"
test = "src"
libs = [".turnkey/soldeps/vendor"]
out = "out"
auto_detect_remappings = false
optimizer = true
optimizer_runs = 200

[fuzz]
runs = 256

[dependencies]
forge-std = { version = "1.8.0", git = "https://github.com/foundry-rs/forge-std", tag = "v1.8.0" }
"""


class CheckFoundryConfigsTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)

    def write(self, rel: str, content: str) -> Path:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def test_root_config_alone_passes(self) -> None:
        self.write("foundry.toml", ROOT_CONFIG)
        self.assertEqual(check_foundry_configs(self.root), [])

    def test_no_config_passes(self) -> None:
        self.assertEqual(check_foundry_configs(self.root), [])

    def test_nested_config_fails(self) -> None:
        self.write("foundry.toml", ROOT_CONFIG)
        self.write(
            "src/examples/hello/foundry.toml", '[profile.default]\nsrc = "src"\n'
        )
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("src/examples/hello/foundry.toml", errors[0])
        self.assertIn("root", errors[0])

    def test_nested_config_fails_without_root_config(self) -> None:
        self.write("src/foundry.toml", "[profile.default]\n")
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("src/foundry.toml", errors[0])

    def test_nested_config_in_skipped_directories_is_ignored(self) -> None:
        self.write("foundry.toml", ROOT_CONFIG)
        for skipped in (
            "node_modules/pkg",
            "buck-out/v2/gen",
            ".turnkey/soldeps/vendor/forge-std",
            ".devenv/state",
            "target/debug",
        ):
            self.write(f"{skipped}/foundry.toml", 'solc_version = "0.8.0"\n')
        self.assertEqual(check_foundry_configs(self.root), [])

    def test_nested_dependencies_no_longer_compared_with_root(self) -> None:
        # Only the nested file itself is reported, not its [dependencies].
        self.write("foundry.toml", ROOT_CONFIG)
        self.write(
            "src/x/foundry.toml",
            '[dependencies]\nsolmate = "https://github.com/transmissions11/solmate@v7"\n',
        )
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertNotIn("solmate", errors[0])

    def test_solc_version_key_fails(self) -> None:
        self.write(
            "foundry.toml", ROOT_CONFIG + '\n[profile.ci]\nsolc_version = "0.8.34"\n'
        )
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("solc_version", errors[0])
        self.assertIn("profile.ci", errors[0])

    def test_solc_key_fails(self) -> None:
        self.write(
            "foundry.toml",
            ROOT_CONFIG.replace(
                "[profile.default]\n", '[profile.default]\nsolc = "solc"\n'
            ),
        )
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("'solc'", errors[0])
        self.assertIn("profile.default", errors[0])

    def test_top_level_solc_version_fails(self) -> None:
        self.write("foundry.toml", 'solc_version = "0.8.34"\n' + ROOT_CONFIG)
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("solc_version", errors[0])

    def test_nested_config_with_solc_version_reports_both(self) -> None:
        self.write("foundry.toml", ROOT_CONFIG)
        self.write("src/x/foundry.toml", '[profile.default]\nsolc_version = "0.8.33"\n')
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 2)

    def test_unparseable_config_fails(self) -> None:
        self.write("foundry.toml", "[profile.default\n")
        errors = check_foundry_configs(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("foundry.toml", errors[0])


if __name__ == "__main__":
    unittest.main()
