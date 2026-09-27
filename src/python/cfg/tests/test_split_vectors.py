"""
The split test cases src/go/pkg/conditions runs too, so the Go and Python
select() keys can't drift apart. Cases with dimensions other than os and cpu
(Go build tags) are the Go module's alone.

Run with: buck2 test //src/python/cfg:test-split-vectors
"""

import json
import os
import unittest
from pathlib import Path

from turnkey.cfg.platforms import Platforms

# Buck2 passes the exported file; a native run reads it from the tree
VECTORS = Path(
    os.environ.get("TURNKEY_SPLIT_VECTORS")
    or Path(__file__).resolve().parents[3] / "go/pkg/conditions/testdata/split-vectors.json"
)


class TestSharedVectors(unittest.TestCase):
    def test_split(self):
        cases = json.loads(VECTORS.read_text())["cases"]
        platform_only = [case for case in cases if not case["dimensions"]]
        self.assertTrue(platform_only)
        for case in platform_only:
            with self.subTest(case=case["name"]):
                platforms = Platforms.from_json(
                    json.dumps({"settings": case["settings"], "platforms": case["platforms"]})
                )

                def labels(p):
                    values = {"os": p.os, "cpu": p.cpu}
                    return [
                        label
                        for rule in case["labels"]
                        if all(values[dim] == value for dim, value in rule["when"].items())
                        for label in rule["labels"]
                    ]

                common, branches = platforms.split(labels)
                self.assertEqual(common, case["common"])
                self.assertEqual(list(branches.items()), [(b["key"], b["labels"]) for b in case["branches"]])


if __name__ == "__main__":
    unittest.main()
