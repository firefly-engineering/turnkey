"""
Tests for Cargo feature unification utilities.

Run with: buck2 test //python/cargo:test_features
"""

import tempfile
import unittest
from pathlib import Path

from .features import (
    parse_feature_forwarding,
    expand_features,
    load_overrides,
)
from .toml import dep_is_available, feature_enables_unavailable_dep


class TestParseFeatureForwarding(unittest.TestCase):
    """Test cases for parse_feature_forwarding."""

    def test_returns_none_for_simple_feature(self):
        """Returns None for non-forwarding features."""
        self.assertIsNone(parse_feature_forwarding("std"))
        self.assertIsNone(parse_feature_forwarding("alloc"))

    def test_parses_simple_forwarding(self):
        """Parses simple dep/feature syntax."""
        result = parse_feature_forwarding("serde/std")
        self.assertEqual(result, ("serde", "std"))

    def test_parses_optional_dep_forwarding(self):
        """Parses dep?/feature syntax."""
        result = parse_feature_forwarding("serde?/std")
        self.assertEqual(result, ("serde", "std"))

    def test_handles_hyphenated_dep_name(self):
        """Handles hyphens in dependency name."""
        result = parse_feature_forwarding("proc-macro2/proc-macro")
        self.assertEqual(result, ("proc-macro2", "proc-macro"))

    def test_handles_feature_with_slash_in_name(self):
        """Correctly splits on first slash only."""
        result = parse_feature_forwarding("tokio/net/tcp")
        self.assertEqual(result, ("tokio", "net/tcp"))


class TestExpandFeatures(unittest.TestCase):
    """Test cases for expand_features."""

    def test_expands_simple_features(self):
        """Expands a feature that enables others."""
        features = {
            "full": ["parsing", "printing"],
        }
        result = expand_features("test", {"full"}, features)
        self.assertIn("full", result)
        self.assertIn("parsing", result)
        self.assertIn("printing", result)

    def test_handles_nested_expansion(self):
        """Handles nested feature expansion."""
        features = {
            "full": ["core"],
            "core": ["alloc"],
        }
        result = expand_features("test", {"full"}, features)
        self.assertIn("full", result)
        self.assertIn("core", result)
        self.assertIn("alloc", result)

    def test_skips_dep_syntax(self):
        """Skips dep: syntax during expansion."""
        features = {
            "printing": ["dep:quote"],
        }
        result = expand_features("test", {"printing"}, features)
        self.assertIn("printing", result)
        # dep:quote should not be in the result
        self.assertNotIn("dep:quote", result)
        self.assertNotIn("quote", result)

    def test_skips_feature_forwarding(self):
        """Skips feature forwarding syntax."""
        features = {
            "std": ["alloc", "serde/std"],
        }
        result = expand_features("test", {"std"}, features)
        self.assertIn("std", result)
        self.assertIn("alloc", result)
        # Forwarding should not be in result
        self.assertNotIn("serde/std", result)

    def test_handles_default_feature(self):
        """Handles default feature specially."""
        features = {
            "default": ["std", "alloc"],
        }
        result = expand_features("test", {"default"}, features)
        # default itself is not added, but its contents are
        self.assertIn("std", result)
        self.assertIn("alloc", result)
        self.assertNotIn("default", result)

    def test_handles_circular_features(self):
        """Handles circular feature references gracefully."""
        features = {
            "a": ["b"],
            "b": ["a"],  # Circular!
        }
        # Should not infinite loop
        result = expand_features("test", {"a"}, features)
        self.assertIn("a", result)
        self.assertIn("b", result)

    def test_handles_unknown_feature(self):
        """Handles features not in the feature map."""
        features = {}
        result = expand_features("test", {"unknown"}, features)
        self.assertIn("unknown", result)


class TestLoadOverrides(unittest.TestCase):
    """Test cases for load_overrides."""

    def test_returns_empty_for_none(self):
        """Returns empty dict when path is None."""
        self.assertEqual(load_overrides(None), {})

    def test_returns_empty_for_nonexistent_file(self):
        """Returns empty dict when file doesn't exist."""
        self.assertEqual(load_overrides(Path("/nonexistent/path.toml")), {})

    def test_loads_complete_override(self):
        """Loads complete feature override."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
            f.write(
                """
[overrides]
serde = ["std", "derive"]
"""
            )
            f.flush()
            result = load_overrides(Path(f.name))
            self.assertEqual(result["serde"], ["std", "derive"])

    def test_loads_additive_override(self):
        """Loads additive feature override."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
            f.write(
                """
[overrides]
syn = { add = ["printing"] }
"""
            )
            f.flush()
            result = load_overrides(Path(f.name))
            self.assertEqual(result["syn"]["add"], ["printing"])

    def test_loads_subtractive_override(self):
        """Loads subtractive feature override."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
            f.write(
                """
[overrides]
tokio = { remove = ["rt-multi-thread"] }
"""
            )
            f.flush()
            result = load_overrides(Path(f.name))
            self.assertEqual(result["tokio"]["remove"], ["rt-multi-thread"])

    def test_loads_combined_override(self):
        """Loads override with both add and remove."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
            f.write(
                """
[overrides]
tokio = { add = ["sync"], remove = ["rt-multi-thread"] }
"""
            )
            f.flush()
            result = load_overrides(Path(f.name))
            self.assertEqual(result["tokio"]["add"], ["sync"])
            self.assertEqual(result["tokio"]["remove"], ["rt-multi-thread"])


class TestIntegration(unittest.TestCase):
    """Integration tests for feature computation."""

    def test_syn_printing_feature_scenario(self):
        """
        Reproduce the syn/printing feature issue.

        When syn has 'printing = ["dep:quote"]' and quote is available
        as 'quote@1.0.43', the printing feature should be considered valid.
        """
        # Available crates with versioned names
        available = {"quote@1.0.43", "proc-macro2@1.0.105", "unicode-ident@1.0.22"}

        # syn's features definition
        syn_features = {
            "printing": ["dep:quote"],
            "proc-macro": ["proc-macro2/proc-macro", "quote?/proc-macro"],
        }

        # quote should be available (versioned name)
        self.assertTrue(dep_is_available("quote", available))

        # printing feature should NOT enable unavailable dep
        self.assertFalse(
            feature_enables_unavailable_dep("printing", syn_features, available)
        )

    def test_feature_with_multiple_deps(self):
        """Test feature that enables multiple dependencies."""
        features = {
            "full": ["dep:quote", "dep:proc-macro2"],
        }

        # All deps available
        available_all = {"quote@1.0.43", "proc-macro2@1.0.105"}
        self.assertFalse(
            feature_enables_unavailable_dep("full", features, available_all)
        )

        # One dep missing
        available_partial = {"quote@1.0.43"}
        self.assertTrue(
            feature_enables_unavailable_dep("full", features, available_partial)
        )


if __name__ == "__main__":
    unittest.main()
