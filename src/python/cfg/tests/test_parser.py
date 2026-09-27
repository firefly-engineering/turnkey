"""
Tests for cfg() expression parsing and evaluation.

Run with: buck2 test //src/python/cfg:test

These test cases document the expected behavior of the cfg() parser
for various platform-specific dependency expressions from Cargo.toml files.
"""

import unittest

from turnkey.cfg.parser import CfgParser, CfgKey, CfgKeyValue, CfgAll, CfgAny, CfgNot
from turnkey.cfg.evaluator import TargetSpec, evaluate_cfg
from turnkey.cfg.platforms import Platform
from turnkey.cfg.target import classify_target_platforms


class TestCfgParser(unittest.TestCase):
    """Test cases for parsing cfg() expressions."""

    def test_simple_target_os(self):
        """Test parsing simple target_os expression."""
        parser = CfgParser('cfg(target_os = "linux")')
        result = parser.parse()
        self.assertIsNotNone(result)
        self.assertIsInstance(result, CfgKeyValue)
        self.assertEqual(result.key, "target_os")
        self.assertEqual(result.value, "linux")

    def test_any_with_multiple_os(self):
        """Test parsing any() with multiple target_os values."""
        parser = CfgParser('cfg(any(target_os = "linux", target_os = "android"))')
        result = parser.parse()
        self.assertIsNotNone(result)
        self.assertIsInstance(result, CfgAny)
        self.assertEqual(len(result.children), 2)

    def test_all_combinator(self):
        """Test parsing all() combinator."""
        parser = CfgParser('cfg(all(target_os = "linux", target_arch = "x86_64"))')
        result = parser.parse()
        self.assertIsNotNone(result)
        self.assertIsInstance(result, CfgAll)
        self.assertEqual(len(result.children), 2)

    def test_not_combinator(self):
        """Test parsing not() combinator."""
        parser = CfgParser("cfg(not(windows))")
        result = parser.parse()
        self.assertIsNotNone(result)
        self.assertIsInstance(result, CfgNot)
        self.assertTrue(hasattr(result, "child"))

    def test_nested_expression(self):
        """Test parsing nested cfg expression."""
        expr = 'cfg(all(any(target_os = "linux", target_os = "android"), not(windows)))'
        parser = CfgParser(expr)
        result = parser.parse()
        self.assertIsNotNone(result)


class TestCfgEvaluation(unittest.TestCase):
    """Test cases for evaluating cfg() expressions against Linux x86_64."""

    def setUp(self):
        self.target = TargetSpec.linux_x86_64()

    def test_unix_is_true(self):
        """Unix shorthand should be true on Linux."""
        parser = CfgParser("cfg(unix)")
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_windows_is_false(self):
        """Windows shorthand should be false on Linux."""
        parser = CfgParser("cfg(windows)")
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_target_os_linux(self):
        """target_os = linux should be true."""
        parser = CfgParser('cfg(target_os = "linux")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_target_os_windows(self):
        """target_os = windows should be false."""
        parser = CfgParser('cfg(target_os = "windows")')
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_target_arch_x86_64(self):
        """target_arch = x86_64 should be true."""
        parser = CfgParser('cfg(target_arch = "x86_64")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_target_env_gnu(self):
        """target_env = gnu should be true for linux-gnu."""
        parser = CfgParser('cfg(target_env = "gnu")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_target_env_empty(self):
        """target_env = "" should be false for linux-gnu (env is "gnu")."""
        parser = CfgParser('cfg(target_env = "")')
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_unknown_cfg_key_is_false(self):
        """Unknown cfg keys should evaluate to false (not set)."""
        parser = CfgParser('cfg(getrandom_backend = "custom")')
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_unknown_standalone_key_is_false(self):
        """Unknown standalone keys like 'miri' should be false."""
        parser = CfgParser("cfg(miri)")
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))


class TestCfgEvaluationMacOS(unittest.TestCase):
    """Test cases for evaluating cfg() expressions against macOS aarch64."""

    def setUp(self):
        self.target = TargetSpec.macos_aarch64()

    def test_unix_is_true(self):
        """Unix shorthand should be true on macOS."""
        parser = CfgParser("cfg(unix)")
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_windows_is_false(self):
        """Windows shorthand should be false on macOS."""
        parser = CfgParser("cfg(windows)")
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_target_os_macos(self):
        """target_os = macos should be true."""
        parser = CfgParser('cfg(target_os = "macos")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_target_os_linux(self):
        """target_os = linux should be false on macOS."""
        parser = CfgParser('cfg(target_os = "linux")')
        self.assertFalse(evaluate_cfg(parser.parse(), self.target))

    def test_target_vendor_apple(self):
        """target_vendor = apple should be true on macOS."""
        parser = CfgParser('cfg(target_vendor = "apple")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_target_arch_aarch64(self):
        """target_arch = aarch64 should be true on macOS aarch64."""
        parser = CfgParser('cfg(target_arch = "aarch64")')
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))

    def test_not_windows(self):
        """not(windows) should be true on macOS."""
        parser = CfgParser("cfg(not(windows))")
        self.assertTrue(evaluate_cfg(parser.parse(), self.target))


class TestGetrandomCfg(unittest.TestCase):
    """Test the specific cfg expression from getrandom that was causing issues."""

    def setUp(self):
        self.target = TargetSpec.linux_x86_64()

    def test_getrandom_libc_dependency_included(self):
        """
        getrandom's libc dependency uses this complex cfg expression:

        cfg(all(
            any(target_os = "linux", target_os = "android"),
            not(any(
                all(target_os = "linux", target_env = ""),
                getrandom_backend = "custom",
                getrandom_backend = "linux_raw",
                getrandom_backend = "rdrand",
                getrandom_backend = "rndr"
            ))
        ))

        On Linux x86_64 with glibc (target_env = "gnu"), this should be TRUE
        because:
        - any(target_os = "linux", ...) = True
        - not(any(
            all(target_os = "linux", target_env = "") = False (env is "gnu")
            getrandom_backend = "custom" = False (not set)
            ... all others = False (not set)
          )) = not(False) = True
        - all(True, True) = True
        """
        # The full expression from getrandom's Cargo.toml
        expr = """cfg(all(
            any(target_os = "linux", target_os = "android"),
            not(any(
                all(target_os = "linux", target_env = ""),
                getrandom_backend = "custom",
                getrandom_backend = "linux_raw",
                getrandom_backend = "rdrand",
                getrandom_backend = "rndr"
            ))
        ))"""

        parser = CfgParser(expr)
        result = parser.parse()
        self.assertIsNotNone(result)

        # With unknown cfg keys defaulting to False, this should be True
        self.assertTrue(evaluate_cfg(result, self.target))

    def test_target_env_empty_string_parsing(self):
        """
        Empty string values in cfg expressions are parsed as standalone keys.
        This is intentional - cfg(target_env = "") parses as CfgKey("target_env")
        which evaluates to False (unknown standalone key).

        This means musl-specific dependencies (target_env = "") are NOT included
        in our builds, which is fine since we target Linux glibc.
        """
        parser = CfgParser('cfg(target_env = "")')
        result = parser.parse()
        # Empty string causes fallback to CfgKey, not CfgKeyValue
        self.assertIsInstance(result, CfgKey)
        self.assertEqual(result.key, "target_env")


LINUX_X86 = Platform("linux", "x86_64")
LINUX_ARM = Platform("linux", "arm64")
MACOS_X86 = Platform("macos", "x86_64")
MACOS_ARM = Platform("macos", "arm64")
PLATFORMS = [LINUX_X86, LINUX_ARM, MACOS_X86, MACOS_ARM]
LINUX = {LINUX_X86, LINUX_ARM}
MACOS = {MACOS_X86, MACOS_ARM}


class TestClassifyTargetPlatforms(unittest.TestCase):
    """Test classify_target_platforms for multi-platform classification."""

    def classify(self, spec):
        return classify_target_platforms(spec, PLATFORMS)

    def test_linux_only(self):
        self.assertEqual(self.classify('cfg(target_os = "linux")'), LINUX)

    def test_macos_only(self):
        self.assertEqual(self.classify('cfg(target_os = "macos")'), MACOS)

    def test_apple_vendor(self):
        self.assertEqual(self.classify('cfg(target_vendor = "apple")'), MACOS)

    def test_unix_matches_all(self):
        self.assertEqual(self.classify("cfg(unix)"), LINUX | MACOS)

    def test_not_windows_matches_all(self):
        self.assertEqual(self.classify("cfg(not(windows))"), LINUX | MACOS)

    def test_windows_matches_none(self):
        self.assertEqual(self.classify('cfg(target_os = "windows")'), set())

    def test_arch(self):
        self.assertEqual(self.classify('cfg(target_arch = "aarch64")'), {LINUX_ARM, MACOS_ARM})

    def test_complex_apple_cfg(self):
        result = self.classify(
            'cfg(all(target_vendor = "apple", any(target_os = "ios", target_os = "macos")))'
        )
        self.assertEqual(result, MACOS)

    def test_target_triple_is_exact(self):
        self.assertEqual(self.classify("x86_64-unknown-linux-gnu"), {LINUX_X86})
        self.assertEqual(self.classify("aarch64-apple-darwin"), {MACOS_ARM})
        self.assertEqual(self.classify("x86_64-unknown-linux-musl"), set())
        self.assertEqual(self.classify("x86_64-pc-windows-msvc"), set())


if __name__ == "__main__":
    unittest.main()
