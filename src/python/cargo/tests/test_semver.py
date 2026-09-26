"""
Tests for Cargo version requirement matching.

Run with: tk test //src/python/cargo:test_semver
"""

import unittest

from turnkey.cargo.semver import best_match, matches


class TestMatches(unittest.TestCase):
    """Cargo's requirement semantics, one operator at a time."""

    def assert_matches(self, req, *versions):
        for v in versions:
            self.assertTrue(matches(req, v), f"{req!r} should match {v}")

    def assert_rejects(self, req, *versions):
        for v in versions:
            self.assertFalse(matches(req, v), f"{req!r} should reject {v}")

    def test_bare_version_is_caret(self):
        self.assert_matches("1.2.3", "1.2.3", "1.9.0")
        self.assert_rejects("1.2.3", "1.2.2", "2.0.0")

    def test_caret_on_zero_major_pins_minor(self):
        self.assert_matches("0.3", "0.3.0", "0.3.31")
        self.assert_rejects("0.3", "0.1.31", "0.4.0")
        self.assert_matches("^0.2.3", "0.2.3", "0.2.9")
        self.assert_rejects("^0.2.3", "0.2.2", "0.3.0")

    def test_caret_on_zero_minor_pins_patch(self):
        self.assert_matches("^0.0.3", "0.0.3")
        self.assert_rejects("^0.0.3", "0.0.4")
        self.assert_matches("0.0", "0.0.7")
        self.assert_rejects("0.0", "0.1.0")

    def test_major_only(self):
        self.assert_matches("1", "1.0.0", "1.99.0")
        self.assert_rejects("1", "0.9.0", "2.0.0")
        self.assert_matches("0", "0.0.1", "0.9.0")
        self.assert_rejects("0", "1.0.0")

    def test_tilde(self):
        self.assert_matches("~1.2.3", "1.2.3", "1.2.9")
        self.assert_rejects("~1.2.3", "1.3.0", "1.2.2")
        self.assert_matches("~1", "1.5.0")
        self.assert_rejects("~1", "2.0.0")

    def test_exact(self):
        self.assert_matches("=1.2.3", "1.2.3")
        self.assert_rejects("=1.2.3", "1.2.4")
        self.assert_matches("=1.2", "1.2.7")
        self.assert_rejects("=1.2", "1.3.0")

    def test_comparisons(self):
        self.assert_matches(">=1.2", "1.2.0", "3.0.0")
        self.assert_rejects(">=1.2", "1.1.9")
        self.assert_matches(">1.2", "1.3.0")
        self.assert_rejects(">1.2", "1.2.9")
        self.assert_matches("<1.2", "1.1.9")
        self.assert_rejects("<1.2", "1.2.0")
        self.assert_matches("<=1.2", "1.2.9")
        self.assert_rejects("<=1.2", "1.3.0")

    def test_wildcards(self):
        self.assert_matches("*", "0.1.0", "5.0.0")
        self.assert_matches("1.*", "1.4.0")
        self.assert_rejects("1.*", "2.0.0")
        self.assert_matches("1.2.x", "1.2.5")
        self.assert_rejects("1.2.x", "1.3.0")

    def test_comma_joins_comparators(self):
        self.assert_matches(">=0.2, <0.4", "0.2.0", "0.3.9")
        self.assert_rejects(">=0.2, <0.4", "0.4.0", "0.1.0")

    def test_prerelease_needs_opt_in(self):
        self.assert_rejects("1", "1.1.0-rc.1")
        self.assert_matches("=1.0.0-rc.1", "1.0.0-rc.1")

    def test_build_metadata_is_ignored(self):
        self.assert_matches("0.3", "0.3.1+wasi-0.2.4")

    def test_whitespace_after_operator(self):
        self.assert_matches(">= 1.2", "1.5.0")


class TestBestMatch(unittest.TestCase):
    def test_picks_highest_matching(self):
        self.assertEqual(best_match("0.3", ["0.3.1", "0.3.31", "0.1.31"]), "0.3.31")

    def test_none_when_nothing_matches(self):
        self.assertIsNone(best_match("0.1", ["0.3.31"]))

    def test_no_requirement_means_any(self):
        self.assertEqual(best_match(None, ["1.0.0", "2.0.0"]), "2.0.0")

    def test_compares_numerically(self):
        self.assertEqual(best_match("1", ["1.9.0", "1.10.0"]), "1.10.0")


if __name__ == "__main__":
    unittest.main()
