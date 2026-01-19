"""
Tests for cfg() expression parsing and evaluation.

Run with: python3 -m pytest python/cfg/test_parser.py -v

These test cases document the expected behavior of the cfg() parser
for various platform-specific dependency expressions from Cargo.toml files.
"""

from .parser import CfgParser, CfgKey, CfgKeyValue, CfgAll, CfgAny, CfgNot
from .evaluator import TargetSpec, evaluate_cfg
from .target import is_linux_compatible_target


class TestCfgParser:
    """Test cases for parsing cfg() expressions."""

    def test_simple_target_os(self):
        """Test parsing simple target_os expression."""
        parser = CfgParser('cfg(target_os = "linux")')
        result = parser.parse()
        assert result is not None
        assert isinstance(result, CfgKeyValue)
        assert result.key == "target_os"
        assert result.value == "linux"

    def test_any_with_multiple_os(self):
        """Test parsing any() with multiple target_os values."""
        parser = CfgParser('cfg(any(target_os = "linux", target_os = "android"))')
        result = parser.parse()
        assert result is not None
        assert isinstance(result, CfgAny)
        assert len(result.children) == 2

    def test_all_combinator(self):
        """Test parsing all() combinator."""
        parser = CfgParser('cfg(all(target_os = "linux", target_arch = "x86_64"))')
        result = parser.parse()
        assert result is not None
        assert isinstance(result, CfgAll)
        assert len(result.children) == 2

    def test_not_combinator(self):
        """Test parsing not() combinator."""
        parser = CfgParser("cfg(not(windows))")
        result = parser.parse()
        assert result is not None
        assert isinstance(result, CfgNot)
        assert hasattr(result, "child")

    def test_nested_expression(self):
        """Test parsing nested cfg expression."""
        expr = 'cfg(all(any(target_os = "linux", target_os = "android"), not(windows)))'
        parser = CfgParser(expr)
        result = parser.parse()
        assert result is not None


class TestCfgEvaluation:
    """Test cases for evaluating cfg() expressions against Linux x86_64."""

    def setup_method(self):
        self.target = TargetSpec.linux_x86_64()

    def test_unix_is_true(self):
        """Unix shorthand should be true on Linux."""
        parser = CfgParser("cfg(unix)")
        assert evaluate_cfg(parser.parse(), self.target) is True

    def test_windows_is_false(self):
        """Windows shorthand should be false on Linux."""
        parser = CfgParser("cfg(windows)")
        assert evaluate_cfg(parser.parse(), self.target) is False

    def test_target_os_linux(self):
        """target_os = linux should be true."""
        parser = CfgParser('cfg(target_os = "linux")')
        assert evaluate_cfg(parser.parse(), self.target) is True

    def test_target_os_windows(self):
        """target_os = windows should be false."""
        parser = CfgParser('cfg(target_os = "windows")')
        assert evaluate_cfg(parser.parse(), self.target) is False

    def test_target_arch_x86_64(self):
        """target_arch = x86_64 should be true."""
        parser = CfgParser('cfg(target_arch = "x86_64")')
        assert evaluate_cfg(parser.parse(), self.target) is True

    def test_target_env_gnu(self):
        """target_env = gnu should be true for linux-gnu."""
        parser = CfgParser('cfg(target_env = "gnu")')
        assert evaluate_cfg(parser.parse(), self.target) is True

    def test_target_env_empty(self):
        """target_env = "" should be false for linux-gnu (env is "gnu")."""
        parser = CfgParser('cfg(target_env = "")')
        assert evaluate_cfg(parser.parse(), self.target) is False

    def test_unknown_cfg_key_is_false(self):
        """Unknown cfg keys should evaluate to false (not set)."""
        parser = CfgParser('cfg(getrandom_backend = "custom")')
        assert evaluate_cfg(parser.parse(), self.target) is False

    def test_unknown_standalone_key_is_false(self):
        """Unknown standalone keys like 'miri' should be false."""
        parser = CfgParser("cfg(miri)")
        assert evaluate_cfg(parser.parse(), self.target) is False


class TestGetrandomCfg:
    """Test the specific cfg expression from getrandom that was causing issues."""

    def setup_method(self):
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
        assert result is not None

        # With unknown cfg keys defaulting to False, this should be True
        assert evaluate_cfg(result, self.target) is True

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
        assert isinstance(result, CfgKey)
        assert result.key == "target_env"


class TestLinuxCompatibleTarget:
    """Test the is_linux_compatible_target function."""

    def test_simple_linux_cfg(self):
        assert is_linux_compatible_target('cfg(target_os = "linux")') is True

    def test_windows_cfg(self):
        assert is_linux_compatible_target('cfg(target_os = "windows")') is False

    def test_unix_cfg(self):
        assert is_linux_compatible_target("cfg(unix)") is True

    def test_wasm_cfg(self):
        assert is_linux_compatible_target('cfg(target_arch = "wasm32")') is False

    def test_any_linux_android(self):
        assert (
            is_linux_compatible_target(
                'cfg(any(target_os = "linux", target_os = "android"))'
            )
            is True
        )

    def test_not_unix(self):
        """not(unix) should be false on Linux."""
        assert is_linux_compatible_target("cfg(not(unix))") is False


if __name__ == "__main__":
    # Run a quick sanity check
    import traceback

    tests = [
        TestCfgParser(),
        TestCfgEvaluation(),
        TestGetrandomCfg(),
        TestLinuxCompatibleTarget(),
    ]

    passed = 0
    failed = 0

    for test_class in tests:
        if hasattr(test_class, "setup_method"):
            test_class.setup_method()

        for name in dir(test_class):
            if name.startswith("test_"):
                try:
                    getattr(test_class, name)()
                    print(f"  PASS: {test_class.__class__.__name__}.{name}")
                    passed += 1
                except AssertionError:
                    print(f"  FAIL: {test_class.__class__.__name__}.{name}")
                    traceback.print_exc()
                    failed += 1
                except Exception as e:
                    print(f"  ERROR: {test_class.__class__.__name__}.{name}: {e}")
                    traceback.print_exc()
                    failed += 1

    print(f"\nResults: {passed} passed, {failed} failed")
    exit(0 if failed == 0 else 1)
