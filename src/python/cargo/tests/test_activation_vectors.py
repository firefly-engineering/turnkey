"""
The feature activation cases src/go/pkg/cargofeatures runs too, so the Go
and Python implementations can't drift apart.

Run with: buck2 test //src/python/cargo:test-activation-vectors
"""

import json
import os
import unittest
from pathlib import Path

from turnkey.cargo.features import activate

# Buck2 passes the exported file; a native run reads it from the tree
VECTORS = Path(
    os.environ.get("TURNKEY_ACTIVATION_VECTORS")
    or Path(__file__).resolve().parents[3]
    / "go/pkg/cargofeatures/testdata/activation-vectors.json"
)


class TestSharedVectors(unittest.TestCase):
    def test_activation(self):
        for case in json.loads(VECTORS.read_text())["cases"]:
            with self.subTest(case["name"]):
                deps = {key: {"version": "1", "optional": True} for key in case["optional"]}
                deps.update({key: {"version": "1"} for key in case["required"] if key not in deps})
                # A key optional in one table and required in another
                target = {key: {"version": "1"} for key in case["required"] if key in case["optional"]}
                cargo = {"features": case["features"], "dependencies": deps}
                if target:
                    cargo["target"] = {"cfg(unix)": {"dependencies": target}}
                got = activate(cargo, case["requested"], frozenset(case["remove"]))
                want = case["want"]
                self.assertEqual(got.features, set(want["features"]))
                self.assertEqual(got.optional_deps, set(want["optional_deps"]))
                self.assertEqual(
                    got.dep_features, {k: set(v) for k, v in want["dep_features"].items()}
                )


if __name__ == "__main__":
    unittest.main()
