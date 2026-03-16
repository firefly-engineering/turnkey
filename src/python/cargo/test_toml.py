"""
Tests for Cargo.toml parsing utilities.

Run with: buck2 test //python/cargo:test_toml
"""

import tempfile
import unittest
from pathlib import Path

from python.cargo.toml import (
    parse_cargo_toml,
    get_crate_name,
    find_workspace_root,
    get_edition,
    get_lib_path,
    is_proc_macro,
    get_optional_deps,
    normalize_crate_name,
    dep_is_available,
    feature_enables_unavailable_dep,
    get_default_features,
    get_cargo_env,
    get_version_req,
    extract_dep_features,
    get_dep_package_name,
)


class TestGetCrateName(unittest.TestCase):
    """Test cases for get_crate_name."""

    def test_returns_package_name(self):
        """Returns name from package section."""
        cargo = {"package": {"name": "my-crate"}}
        self.assertEqual(get_crate_name(cargo, "fallback"), "my-crate")

    def test_returns_fallback_when_no_package(self):
        """Returns fallback when no package section."""
        cargo = {}
        self.assertEqual(get_crate_name(cargo, "fallback"), "fallback")

    def test_returns_fallback_when_no_name(self):
        """Returns fallback when name is missing."""
        cargo = {"package": {}}
        self.assertEqual(get_crate_name(cargo, "fallback"), "fallback")


class TestFindWorkspaceRoot(unittest.TestCase):
    """Test cases for find_workspace_root."""

    def test_finds_workspace_in_parent(self):
        """Finds workspace root in parent directory."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            # Create workspace root Cargo.toml
            (tmpdir / "Cargo.toml").write_text(
                """
[workspace]
members = ["crates/*"]

[workspace.package]
edition = "2024"
"""
            )
            # Create a member crate directory
            member = tmpdir / "crates" / "my-crate"
            member.mkdir(parents=True)

            result = find_workspace_root(member)
            self.assertEqual(result, tmpdir.resolve())

    def test_returns_none_when_no_workspace(self):
        """Returns None when no workspace found."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            # Create a non-workspace Cargo.toml
            (tmpdir / "Cargo.toml").write_text(
                """
[package]
name = "standalone"
"""
            )
            result = find_workspace_root(tmpdir)
            self.assertIsNone(result)


class TestGetEdition(unittest.TestCase):
    """Test cases for get_edition."""

    def test_returns_explicit_edition(self):
        """Returns edition from Cargo.toml."""
        cargo = {"package": {"edition": "2021"}}
        self.assertEqual(get_edition(cargo), "2021")

    def test_defaults_to_2015(self):
        """Defaults to 2015 when not specified."""
        cargo = {}
        self.assertEqual(get_edition(cargo), "2015")

    def test_defaults_when_package_empty(self):
        """Defaults when package section exists but edition missing."""
        cargo = {"package": {"name": "foo"}}
        self.assertEqual(get_edition(cargo), "2015")

    def test_workspace_inheritance_with_workspace_cargo(self):
        """Resolves edition from workspace_cargo when using inheritance."""
        cargo = {"package": {"name": "member", "edition": {"workspace": True}}}
        workspace_cargo = {"workspace": {"package": {"edition": "2024"}}}
        self.assertEqual(get_edition(cargo, workspace_cargo=workspace_cargo), "2024")

    def test_workspace_inheritance_auto_find(self):
        """Auto-finds workspace root when crate_dir is provided."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            # Create workspace root
            (tmpdir / "Cargo.toml").write_text(
                """
[workspace]
members = ["crates/*"]

[workspace.package]
edition = "2024"
"""
            )
            # Create member crate
            member = tmpdir / "crates" / "my-crate"
            member.mkdir(parents=True)
            (member / "Cargo.toml").write_text(
                """
[package]
name = "my-crate"
edition.workspace = true
"""
            )

            cargo = {"package": {"name": "my-crate", "edition": {"workspace": True}}}
            self.assertEqual(get_edition(cargo, crate_dir=member), "2024")

    def test_workspace_inheritance_fallback(self):
        """Falls back to 2021 when workspace inheritance can't be resolved."""
        cargo = {"package": {"name": "orphan", "edition": {"workspace": True}}}
        # No workspace_cargo and no crate_dir - can't resolve
        self.assertEqual(get_edition(cargo), "2021")


class TestGetLibPath(unittest.TestCase):
    """Test cases for get_lib_path."""

    def test_returns_explicit_lib_path(self):
        """Returns explicit path from [lib] section."""
        cargo = {"lib": {"path": "custom/lib.rs"}}
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(get_lib_path(cargo, Path(tmpdir)), "custom/lib.rs")

    def test_returns_src_lib_when_exists(self):
        """Returns src/lib.rs when it exists."""
        cargo = {}
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / "src").mkdir()
            (tmpdir / "src" / "lib.rs").touch()
            self.assertEqual(get_lib_path(cargo, tmpdir), "src/lib.rs")

    def test_returns_lib_rs_when_exists(self):
        """Returns lib.rs in crate root when it exists."""
        cargo = {}
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / "lib.rs").touch()
            self.assertEqual(get_lib_path(cargo, tmpdir), "lib.rs")

    def test_returns_none_when_no_lib(self):
        """Returns None when no library source found."""
        cargo = {}
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertIsNone(get_lib_path(cargo, Path(tmpdir)))


class TestIsProcMacro(unittest.TestCase):
    """Test cases for is_proc_macro."""

    def test_returns_true_for_proc_macro(self):
        """Returns True when proc-macro is set."""
        cargo = {"lib": {"proc-macro": True}}
        self.assertTrue(is_proc_macro(cargo))

    def test_returns_false_when_not_set(self):
        """Returns False when not a proc-macro."""
        cargo = {"lib": {}}
        self.assertFalse(is_proc_macro(cargo))

    def test_returns_false_when_no_lib_section(self):
        """Returns False when no [lib] section."""
        cargo = {}
        self.assertFalse(is_proc_macro(cargo))


class TestGetOptionalDeps(unittest.TestCase):
    """Test cases for get_optional_deps."""

    def test_finds_optional_deps(self):
        """Finds optional dependencies."""
        cargo = {
            "dependencies": {
                "serde": {"version": "1.0", "optional": True},
                "tokio": "1.0",  # Not optional
            }
        }
        result = get_optional_deps(cargo)
        self.assertIn("serde", result)
        self.assertNotIn("tokio", result)

    def test_includes_package_alias(self):
        """Includes both alias and package name."""
        cargo = {
            "dependencies": {
                "my_serde": {"package": "serde", "version": "1.0", "optional": True}
            }
        }
        result = get_optional_deps(cargo)
        self.assertIn("serde", result)
        self.assertIn("my_serde", result)

    def test_returns_empty_set_when_no_deps(self):
        """Returns empty set when no dependencies."""
        cargo = {}
        self.assertEqual(get_optional_deps(cargo), set())


class TestNormalizeCrateName(unittest.TestCase):
    """Test cases for normalize_crate_name."""

    def test_replaces_hyphens_with_underscores(self):
        """Hyphens become underscores."""
        self.assertEqual(normalize_crate_name("my-crate"), "my_crate")

    def test_leaves_underscores_alone(self):
        """Underscores stay as underscores."""
        self.assertEqual(normalize_crate_name("my_crate"), "my_crate")

    def test_handles_multiple_hyphens(self):
        """Multiple hyphens all become underscores."""
        self.assertEqual(normalize_crate_name("a-b-c"), "a_b_c")


class TestDepIsAvailable(unittest.TestCase):
    """Test cases for dep_is_available."""

    def test_exact_match(self):
        """Finds exact matches."""
        available = {"serde", "tokio"}
        self.assertTrue(dep_is_available("serde", available))
        self.assertFalse(dep_is_available("missing", available))

    def test_hyphen_underscore_variants(self):
        """Handles hyphen/underscore normalization."""
        available = {"my_crate"}
        self.assertTrue(dep_is_available("my-crate", available))
        self.assertTrue(dep_is_available("my_crate", available))

    def test_versioned_crate_names(self):
        """Handles versioned crate names like 'quote@1.0.43'."""
        available = {"quote@1.0.43", "proc-macro2@1.0.105"}
        self.assertTrue(dep_is_available("quote", available))
        self.assertTrue(dep_is_available("proc-macro2", available))
        self.assertTrue(dep_is_available("proc_macro2", available))
        self.assertFalse(dep_is_available("missing", available))

    def test_mixed_versioned_and_unversioned(self):
        """Works with mix of versioned and unversioned names."""
        available = {"serde", "tokio@1.0.0"}
        self.assertTrue(dep_is_available("serde", available))
        self.assertTrue(dep_is_available("tokio", available))

    def test_underscore_in_versioned_name(self):
        """Handles underscores in versioned names."""
        available = {"proc_macro2@1.0.105"}
        self.assertTrue(dep_is_available("proc-macro2", available))


class TestFeatureEnablesUnavailableDep(unittest.TestCase):
    """Test cases for feature_enables_unavailable_dep."""

    def test_returns_false_for_unknown_feature(self):
        """Returns False for features not in the dict."""
        features = {}
        available = {"serde"}
        self.assertFalse(feature_enables_unavailable_dep("unknown", features, available))

    def test_returns_false_for_available_dep(self):
        """Returns False when dep: enables an available dep."""
        features = {"printing": ["dep:quote"]}
        available = {"quote@1.0.43"}
        self.assertFalse(feature_enables_unavailable_dep("printing", features, available))

    def test_returns_true_for_unavailable_dep(self):
        """Returns True when dep: enables an unavailable dep."""
        features = {"printing": ["dep:quote"]}
        available = {"serde"}
        self.assertTrue(feature_enables_unavailable_dep("printing", features, available))

    def test_ignores_non_dep_items(self):
        """Ignores feature items that aren't dep: syntax."""
        features = {"full": ["parsing", "printing"]}
        available = set()
        # No dep: items, so should return False
        self.assertFalse(feature_enables_unavailable_dep("full", features, available))


class TestGetDefaultFeatures(unittest.TestCase):
    """Test cases for get_default_features."""

    def test_expands_default_features(self):
        """Expands default feature list."""
        cargo = {
            "features": {
                "default": ["std", "alloc"],
            }
        }
        result = get_default_features(cargo, set())
        self.assertIn("std", result)
        self.assertIn("alloc", result)

    def test_expands_nested_features(self):
        """Expands feature that enables other features."""
        cargo = {
            "features": {
                "default": ["full"],
                "full": ["parsing", "printing"],
            }
        }
        result = get_default_features(cargo, set())
        self.assertIn("full", result)
        self.assertIn("parsing", result)
        self.assertIn("printing", result)

    def test_filters_feature_forwarding(self):
        """Filters out feature forwarding syntax."""
        cargo = {
            "features": {
                "default": ["std", "serde/std"],
            }
        }
        result = get_default_features(cargo, set())
        self.assertIn("std", result)
        self.assertNotIn("serde/std", result)

    def test_filters_unavailable_optional_deps(self):
        """Filters features for unavailable optional deps."""
        cargo = {
            "dependencies": {
                "serde": {"version": "1.0", "optional": True},
            },
            "features": {
                "default": ["std", "serde"],
            },
        }
        result = get_default_features(cargo, set())  # serde not available
        self.assertIn("std", result)
        self.assertNotIn("serde", result)

    def test_includes_available_optional_deps(self):
        """Includes features for available optional deps."""
        cargo = {
            "dependencies": {
                "serde": {"version": "1.0", "optional": True},
            },
            "features": {
                "default": ["std", "serde"],
            },
        }
        result = get_default_features(cargo, {"serde@1.0.0"})
        self.assertIn("std", result)
        self.assertIn("serde", result)


class TestGetCargoEnv(unittest.TestCase):
    """Test cases for get_cargo_env."""

    def test_basic_version_parsing(self):
        """Parses simple version string."""
        cargo = {"package": {"name": "test", "version": "1.2.3"}}
        env = get_cargo_env(cargo, "test")
        self.assertEqual(env["CARGO_PKG_VERSION"], "1.2.3")
        self.assertEqual(env["CARGO_PKG_VERSION_MAJOR"], "1")
        self.assertEqual(env["CARGO_PKG_VERSION_MINOR"], "2")
        self.assertEqual(env["CARGO_PKG_VERSION_PATCH"], "3")
        self.assertEqual(env["CARGO_PKG_VERSION_PRE"], "")

    def test_prerelease_version(self):
        """Parses prerelease version."""
        cargo = {"package": {"version": "1.0.0-beta.1"}}
        env = get_cargo_env(cargo, "test")
        self.assertEqual(env["CARGO_PKG_VERSION"], "1.0.0-beta.1")
        self.assertEqual(env["CARGO_PKG_VERSION_PRE"], "beta.1")

    def test_includes_optional_fields(self):
        """Includes optional package fields."""
        cargo = {
            "package": {
                "version": "1.0.0",
                "description": "A test crate",
                "license": "MIT",
                "repository": "https://github.com/test/test",
                "authors": ["Alice", "Bob"],
            }
        }
        env = get_cargo_env(cargo, "test")
        self.assertEqual(env["CARGO_PKG_DESCRIPTION"], "A test crate")
        self.assertEqual(env["CARGO_PKG_LICENSE"], "MIT")
        self.assertEqual(env["CARGO_PKG_REPOSITORY"], "https://github.com/test/test")
        self.assertEqual(env["CARGO_PKG_AUTHORS"], "Alice:Bob")

    def test_defaults_for_missing_version(self):
        """Defaults to 0.0.0 when version missing."""
        cargo = {"package": {}}
        env = get_cargo_env(cargo, "test")
        self.assertEqual(env["CARGO_PKG_VERSION"], "0.0.0")


class TestGetVersionReq(unittest.TestCase):
    """Test cases for get_version_req."""

    def test_string_version(self):
        """Extracts version from string spec."""
        self.assertEqual(get_version_req("1.0"), "1.0")

    def test_dict_version(self):
        """Extracts version from dict spec."""
        self.assertEqual(get_version_req({"version": "1.0", "features": ["std"]}), "1.0")

    def test_missing_version(self):
        """Returns None when version missing."""
        self.assertIsNone(get_version_req({"path": "../local"}))
        self.assertIsNone(get_version_req(123))


class TestExtractDepFeatures(unittest.TestCase):
    """Test cases for extract_dep_features."""

    def test_string_spec_empty_features(self):
        """String specs have no explicit features."""
        self.assertEqual(extract_dep_features("1.0"), [])

    def test_dict_with_features(self):
        """Extracts features from dict spec."""
        result = extract_dep_features({"version": "1.0", "features": ["std", "alloc"]})
        self.assertIn("std", result)
        self.assertIn("alloc", result)
        self.assertIn("default", result)  # default-features not disabled

    def test_dict_without_default_features(self):
        """Handles default-features = false."""
        result = extract_dep_features(
            {"version": "1.0", "features": ["std"], "default-features": False}
        )
        self.assertIn("std", result)
        self.assertNotIn("default", result)

    def test_dict_default_features_underscore(self):
        """Handles default_features (underscore variant)."""
        result = extract_dep_features(
            {"version": "1.0", "features": ["std"], "default_features": False}
        )
        self.assertNotIn("default", result)


class TestGetDepPackageName(unittest.TestCase):
    """Test cases for get_dep_package_name."""

    def test_returns_dep_name_for_string(self):
        """Returns dep name for string spec."""
        self.assertEqual(get_dep_package_name("serde", "1.0"), "serde")

    def test_returns_dep_name_when_no_package(self):
        """Returns dep name when no package override."""
        self.assertEqual(get_dep_package_name("serde", {"version": "1.0"}), "serde")

    def test_returns_package_name_for_rename(self):
        """Returns package name for renamed deps."""
        self.assertEqual(
            get_dep_package_name("my_serde", {"package": "serde", "version": "1.0"}),
            "serde",
        )


class TestParseCargoToml(unittest.TestCase):
    """Test cases for parse_cargo_toml."""

    def test_parses_valid_cargo_toml(self):
        """Parses a valid Cargo.toml file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            cargo_toml = tmpdir / "Cargo.toml"
            cargo_toml.write_text(
                """
[package]
name = "test-crate"
version = "1.0.0"
edition = "2021"

[dependencies]
serde = "1.0"
"""
            )
            result = parse_cargo_toml(tmpdir)
            self.assertEqual(result["package"]["name"], "test-crate")
            self.assertEqual(result["package"]["version"], "1.0.0")
            self.assertIn("serde", result["dependencies"])

    def test_returns_empty_dict_when_not_exists(self):
        """Returns empty dict when Cargo.toml doesn't exist."""
        with tempfile.TemporaryDirectory() as tmpdir:
            result = parse_cargo_toml(Path(tmpdir))
            self.assertEqual(result, {})


if __name__ == "__main__":
    unittest.main()
