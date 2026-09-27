"""
The cfg() test cases src/go/pkg/cargocfg runs too, so the Go and Python
evaluators can't drift apart.

Run with: buck2 test //src/python/cfg:test-vectors
"""

import json
import os
import unittest
from pathlib import Path

from turnkey.cfg.evaluator import TargetSpec, evaluate_cfg
from turnkey.cfg.parser import CfgParser

# Buck2 passes the exported file; a native run reads it from the tree
VECTORS = Path(
    os.environ.get("TURNKEY_CFG_VECTORS")
    or Path(__file__).resolve().parents[3] / "go/pkg/cargocfg/testdata/cfg-vectors.json"
)


class TestSharedVectors(unittest.TestCase):
    def test_cfg_expressions(self):
        vectors = json.loads(VECTORS.read_text())
        platforms = {
            f"{p['os']}-{p['cpu']}": TargetSpec.from_platform(p["os"], p["cpu"])
            for p in vectors["platforms"]
        }
        for case in vectors["cfg"]:
            with self.subTest(spec=case["spec"]):
                predicate = CfgParser(case["spec"]).parse()
                self.assertIsNotNone(predicate)
                matches = {name for name, spec in platforms.items() if evaluate_cfg(predicate, spec)}
                self.assertEqual(matches, set(case["matches"]))

    def test_unknown_platform(self):
        with self.assertRaises(ValueError):
            TargetSpec.from_platform("windows", "x86_64")


if __name__ == "__main__":
    unittest.main()
