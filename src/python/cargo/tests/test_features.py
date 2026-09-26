"""
Tests for Cargo feature unification utilities.

Run with: buck2 test //python/cargo:test_features
"""

import tempfile
import unittest
from pathlib import Path

from turnkey.cargo.features import (
    activate,
    compute_unified_features,
    parse_feature_forwarding,
    load_overrides,
    load_requested,
)
from turnkey.cargo.toml import dep_is_available, feature_enables_unavailable_dep


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


class TestActivate(unittest.TestCase):
    """Which features, optional deps and dep features a request turns on."""

    CARGO = {
        "features": {
            "default": ["std", "async-await-macro"],
            "std": ["alloc", "slab"],
            "alloc": ["futures-core/alloc"],
            "async-await-macro": ["dep:futures-macro"],
            "compat": ["dep:futures_01"],
            "io": ["std", "memchr"],
            "sink": ["futures-sink?/std"],
        },
        "dependencies": {
            "futures-core": {"version": "0.3"},
            "futures-macro": {"version": "0.3", "optional": True},
            "futures_01": {"version": "0.1", "package": "futures", "optional": True},
            "futures-sink": {"version": "0.3", "optional": True},
            "memchr": {"version": "2", "optional": True},
            "slab": {"version": "0.4", "optional": True},
        },
    }

    def test_expands_features_transitively(self):
        result = activate(self.CARGO, {"std"})
        self.assertEqual(result.features, {"std", "alloc", "slab"})

    def test_default_is_expanded_but_not_reported(self):
        result = activate(self.CARGO, {"default"})
        self.assertIn("async-await-macro", result.features)
        self.assertNotIn("default", result.features)

    def test_dep_syntax_activates_optional_dep(self):
        self.assertEqual(activate(self.CARGO, {"compat"}).optional_deps, {"futures_01"})

    def test_optional_dep_stays_off_without_its_feature(self):
        self.assertEqual(activate(self.CARGO, {"alloc"}).optional_deps, set())

    def test_implicit_feature_of_optional_dep(self):
        # slab and memchr are never named with dep:, so each has an implicit
        # feature of its own name.
        self.assertEqual(activate(self.CARGO, {"io"}).optional_deps, {"slab", "memchr"})

    def test_forwarded_feature_requested_on_dep(self):
        self.assertEqual(activate(self.CARGO, {"alloc"}).dep_features, {"futures-core": {"alloc"}})

    def test_weak_forwarding_needs_the_dep_active(self):
        self.assertEqual(activate(self.CARGO, {"sink"}).dep_features, {})
        result = activate(self.CARGO, {"sink", "futures-sink"})
        self.assertEqual(result.dep_features, {"futures-sink": {"std"}})

    def test_strong_forwarding_activates_optional_dep(self):
        cargo = {
            "features": {"serde": ["dep:serde", "chrono/serde"]},
            "dependencies": {"chrono": {"version": "0.4", "optional": True}},
        }
        result = activate(cargo, {"serde"})
        self.assertIn("chrono", result.optional_deps)
        self.assertEqual(result.dep_features, {"chrono": {"serde"}})

    def test_strong_forwarding_enables_the_implicit_feature(self):
        # tokio: net = ["mio/os-poll"], with mio never named via dep:
        cargo = {
            "features": {"net": ["mio/os-poll"]},
            "dependencies": {"mio": {"version": "1", "optional": True}},
        }
        self.assertEqual(activate(cargo, {"net"}).features, {"net", "mio"})

    def test_strong_forwarding_enables_the_explicit_feature_of_that_name(self):
        # yoke: derive = ["zerofrom/derive"] also turns on
        # zerofrom = ["dep:zerofrom"]
        cargo = {
            "features": {
                "derive": ["zerofrom/derive"],
                "zerofrom": ["dep:zerofrom"],
            },
            "dependencies": {"zerofrom": {"version": "0.1", "optional": True}},
        }
        self.assertEqual(activate(cargo, {"derive"}).features, {"derive", "zerofrom"})

    def test_removed_features_are_not_expanded(self):
        result = activate(self.CARGO, {"default"}, remove={"async-await-macro"})
        self.assertNotIn("async-await-macro", result.features)
        self.assertEqual(result.optional_deps, {"slab"})

    def test_removing_default_drops_the_default_set(self):
        result = activate(self.CARGO, {"default", "alloc"}, remove={"default"})
        self.assertEqual(result.features, {"alloc"})


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


class TestLoadRequested(unittest.TestCase):
    def load(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rust-deps.toml"
            path.write_text(text)
            return load_requested(path)

    def test_reads_requested_specs(self):
        result = self.load(
            'schema_version = 1\n'
            '[[requested]]\nname = "tokio"\nversion = "1"\n'
            'default-features = false\nfeatures = ["rt"]\n'
        )
        self.assertEqual(
            result,
            [{"name": "tokio", "version": "1", "default-features": False, "features": ["rt"]}],
        )

    def test_none_without_a_requested_section(self):
        self.assertIsNone(self.load('schema_version = 1\n[deps."a@1.0.0"]\nname = "a"\n'))

    def test_none_without_a_file(self):
        self.assertIsNone(load_requested(None))


def write_vendor(root: Path, crates: dict[str, str]) -> Path:
    """Lay out a vendor dir: {"name@version": "Cargo.toml text"}."""
    vendor = root / "vendor"
    for key, manifest in crates.items():
        name, version = key.split("@")
        crate_dir = vendor / key
        crate_dir.mkdir(parents=True)
        (crate_dir / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "{version}"\n' + manifest
        )
    return vendor


FUTURES_UTIL = """
[features]
default = ["std", "async-await", "async-await-macro"]
std = ["alloc", "futures-core/std"]
alloc = ["futures-core/alloc"]
async-await = []
async-await-macro = ["async-await", "futures-macro"]
compat = ["std", "futures_01"]

[dependencies]
futures-core = { version = "0.3.31", default-features = false }
futures-macro = { version = "0.3.31", optional = true }
futures_01 = { version = "0.1.25", package = "futures", optional = true }
"""

FUTURES_CORE = """
[features]
default = ["std"]
std = ["alloc"]
alloc = []
"""


class TestComputeUnifiedFeatures(unittest.TestCase):
    """Cargo-style unification, driven by what the workspace requests."""

    def unify(self, crates, requested, overrides=None):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = write_vendor(Path(tmp), crates)
            return compute_unified_features(vendor, overrides or {}, requested)

    def futures(self, requested, **extra):
        crates = {
            "futures-util@0.3.31": FUTURES_UTIL,
            "futures-core@0.3.31": FUTURES_CORE,
            "futures-macro@0.3.31": "",
            **extra,
        }
        return self.unify(crates, requested)

    def test_defaults_off_when_no_requirer_asks(self):
        result = self.futures(
            [{"name": "futures-util", "version": "0.3", "default-features": False, "features": ["std"]}]
        )
        self.assertEqual(result["futures-util@0.3.31"], ["alloc", "std"])

    def test_defaults_on_when_a_requirer_asks(self):
        result = self.futures([{"name": "futures-util", "version": "0.3"}])
        self.assertIn("async-await-macro", result["futures-util@0.3.31"])

    def test_dependency_defaults_follow_the_dependent(self):
        # futures-util asks futures-core for no defaults, only std via "std".
        result = self.futures(
            [{"name": "futures-util", "version": "0.3", "default-features": False}]
        )
        self.assertEqual(result["futures-util@0.3.31"], [])
        self.assertEqual(result["futures-core@0.3.31"], [])

    def test_forwarded_features_reach_the_dependency(self):
        result = self.futures(
            [{"name": "futures-util", "version": "0.3", "default-features": False, "features": ["std"]}]
        )
        self.assertEqual(result["futures-core@0.3.31"], ["alloc", "std"])

    def test_inactive_optional_dep_requests_nothing(self):
        # futures-macro is optional and only async-await-macro activates it.
        # Nothing else requires it, so it gets its (empty) defaults alone.
        result = self.futures(
            [{"name": "futures-util", "version": "0.3", "default-features": False}],
            **{"futures-macro@0.3.31": '[features]\ndefault = ["x"]\nx = []\n'},
        )
        self.assertEqual(result["futures-macro@0.3.31"], ["x"])
        result = self.futures(
            [
                {"name": "futures-util", "version": "0.3", "default-features": False},
                {"name": "futures-macro", "version": "0.3", "default-features": False},
            ],
            **{"futures-macro@0.3.31": '[features]\ndefault = ["x"]\nx = []\n'},
        )
        self.assertEqual(result["futures-macro@0.3.31"], [])

    def test_renamed_dep_resolves_by_version(self):
        # compat is on, futures_01 wants futures 0.1: it must not feed the
        # futures 0.3 that is vendored.
        futures_03 = '[features]\ndefault = ["std"]\nstd = []\nsecret = []\n'
        result = self.futures(
            [
                {"name": "futures-util", "version": "0.3", "default-features": False, "features": ["compat"]},
                {"name": "futures", "version": "0.3", "default-features": False},
            ],
            **{"futures@0.3.31": futures_03},
        )
        self.assertEqual(result["futures@0.3.31"], [])

    def test_versions_of_a_crate_unify_separately(self):
        crates = {
            "app@1.0.0": '[dependencies]\nrand = { version = "0.8", features = ["small_rng"] }\n',
            "rand@0.8.5": "[features]\nsmall_rng = []\n",
            "rand@0.9.0": "[features]\nsmall_rng = []\n",
        }
        result = self.unify(
            crates,
            [{"name": "app", "version": "1"}, {"name": "rand", "version": "0.9"}],
        )
        self.assertEqual(result["rand@0.8.5"], ["small_rng"])
        self.assertEqual(result["rand@0.9.0"], [])

    def test_weak_forwarding_needs_the_dep_active(self):
        crates = {
            "app@1.0.0": """
[features]
std = ["serde?/std"]
[dependencies]
serde = { version = "1", optional = true, default-features = false }
""",
            "serde@1.0.0": "[features]\nstd = []\n",
        }
        off = self.unify(crates, [{"name": "app", "version": "1", "features": ["std"]}])
        self.assertNotIn("std", off.get("serde@1.0.0", []))
        on = self.unify(crates, [{"name": "app", "version": "1", "features": ["std", "serde"]}])
        self.assertEqual(on["serde@1.0.0"], ["std"])

    def test_non_linux_target_deps_are_ignored(self):
        crates = {
            "app@1.0.0": """
[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["fileapi"] }
""",
            "winapi@0.3.9": "[features]\nfileapi = []\n",
        }
        result = self.unify(crates, [{"name": "app", "version": "1"}])
        self.assertEqual(result["winapi@0.3.9"], [])

    def test_macos_target_deps_are_walked(self):
        # One feature set serves every supported platform, so what the macOS
        # side asks for counts too.
        crates = {
            "app@1.0.0": """
[target.'cfg(target_os = "macos")'.dependencies]
libc = { version = "0.2", default-features = false, features = ["extra_traits"] }
""",
            "libc@0.2.170": '[features]\ndefault = ["std"]\nstd = []\nextra_traits = []\n',
        }
        result = self.unify(crates, [{"name": "app", "version": "1"}])
        self.assertEqual(result["libc@0.2.170"], ["extra_traits"])

    def test_optional_on_one_platform_required_on_another(self):
        # notify: mio is required on Linux, optional (and off) on macOS.
        crates = {
            "notify@8.2.0": """
[target.'cfg(target_os = "linux")'.dependencies]
mio = { version = "1.0" }
[target.'cfg(target_os = "macos")'.dependencies]
mio = { version = "1.0", optional = true }
""",
            "mio@1.1.1": '[features]\ndefault = ["log"]\nlog = []\n',
        }
        result = self.unify(
            crates,
            [
                {"name": "notify", "version": "8"},
                {"name": "mio", "version": "1", "default-features": False},
            ],
        )
        self.assertEqual(result["mio@1.1.1"], ["log"])

    def test_unreached_crate_gets_only_its_defaults(self):
        # cc is only a build-dependency: nothing reaches it, so it gets its
        # defaults, and those must not leak into what it depends on.
        crates = {
            "cc@1.2.0": '[features]\ndefault = ["p"]\np = ["shlex/std"]\n[dependencies]\nshlex = { version = "1", default-features = false }\n',
            "shlex@1.3.0": '[features]\ndefault = ["std"]\nstd = []\n',
        }
        result = self.unify(crates, [{"name": "shlex", "version": "1", "default-features": False}])
        self.assertEqual(result["cc@1.2.0"], ["p"])
        self.assertEqual(result["shlex@1.3.0"], [])

    def test_without_requests_every_crate_gets_its_defaults(self):
        result = self.futures(None)
        self.assertIn("async-await-macro", result["futures-util@0.3.31"])
        self.assertEqual(result["futures-core@0.3.31"], ["alloc", "std"])

    def test_added_override_features_propagate(self):
        crates = {
            "clap@4.5.0": '[features]\nderive = ["dep:clap_derive"]\n[dependencies]\nclap_derive = { version = "4", optional = true, features = ["fancy"] }\n',
            "clap_derive@4.5.0": '[features]\ndefault = []\nfancy = []\n',
        }
        result = self.unify(
            crates,
            [{"name": "clap", "version": "4", "default-features": False}],
            overrides={"clap": {"add": ["derive"]}},
        )
        self.assertEqual(result["clap@4.5.0"], ["derive"])
        # derive activates clap_derive, and with it clap's request for fancy.
        self.assertEqual(result["clap_derive@4.5.0"], ["fancy"])

    def test_removed_override_features_do_not_propagate(self):
        result = self.unify(
            {
                "futures-util@0.3.31": FUTURES_UTIL,
                "futures-core@0.3.31": FUTURES_CORE,
                "futures-macro@0.3.31": '[features]\ndefault = ["x"]\nx = []\n',
            },
            [
                {"name": "futures-util", "version": "0.3"},
                {"name": "futures-macro", "version": "0.3", "default-features": False},
            ],
            overrides={"futures-util": {"remove": ["async-await-macro"]}},
        )
        self.assertNotIn("async-await-macro", result["futures-util@0.3.31"])
        self.assertEqual(result["futures-macro@0.3.31"], [])

    def test_replacement_override_is_final(self):
        result = self.unify(
            {"futures-core@0.3.31": FUTURES_CORE},
            [{"name": "futures-core"}],
            overrides={"futures-core": ["alloc"]},
        )
        self.assertEqual(result["futures-core@0.3.31"], ["alloc"])


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
