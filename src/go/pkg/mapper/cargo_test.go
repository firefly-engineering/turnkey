package mapper

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// writeTree writes files (relative path -> content) under root.
func writeTree(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for rel, content := range files {
		path := filepath.Join(root, rel)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}
}

// cargoWorkspaceFixture is a workspace with two members: app depends on lib
// and on external crates, one of them renamed.
func cargoWorkspaceFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"Cargo.toml": `[workspace]
members = ["crates/*"]

[workspace.dependencies]
anyhow = "1.0"
tree-sitter = "0.25"
json = { package = "serde_json", version = "1.0" }
tempfile = "3"
libc = "0.2"
my-lib = { path = "crates/lib" }
`,
		"rust-deps.toml": `schema_version = 1

[deps."anyhow@1.0.0"]
name = "anyhow"
[deps."tree-sitter@0.25.0"]
name = "tree-sitter"
[deps."serde_json@1.0.0"]
name = "serde_json"
[deps."tempfile@3.0.0"]
name = "tempfile"
[deps."libc@0.2.0"]
name = "libc"
[deps."log@0.4.0"]
name = "log"
`,
		"crates/lib/Cargo.toml": `[package]
name = "my-lib"

[dependencies]
anyhow.workspace = true
`,
		"crates/app/Cargo.toml": `[package]
name = "app"

[dependencies]
anyhow.workspace = true
tree-sitter.workspace = true
json.workspace = true
my-lib.workspace = true
log = "0.4"
unknown-crate = "1"
libc = { workspace = true, optional = true }

[dev-dependencies]
tempfile.workspace = true

[target.'cfg(unix)'.dependencies]
my-lib = { workspace = true }
`,
	})
	return root
}

func targets(deps []MappedDep) []string {
	var out []string
	for _, d := range deps {
		out = append(out, d.Target)
	}
	return out
}

// A crate's external deps are in its language's cell, as listed in its
// language's deps file, wherever sync.toml says they are.
func TestMapRustCrateFromLanguage(t *testing.T) {
	root := cargoWorkspaceFixture(t)
	if err := os.MkdirAll(filepath.Join(root, "third-party"), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(filepath.Join(root, "rust-deps.toml"), filepath.Join(root, "third-party/rust-deps.toml")); err != nil {
		t.Fatal(err)
	}
	m, err := New(Config{
		ProjectRoot: root,
		Languages:   []syncconfig.Language{{Name: "rust", Cell: "crates", DepsFile: "third-party/rust-deps.toml"}},
	})
	if err != nil {
		t.Fatal(err)
	}

	mapping, err := m.Language("rust").ResolveDeps(filepath.Join(root, "crates/lib"), Request{})
	if err != nil {
		t.Fatal(err)
	}
	if got, want := targets(mapping.Deps), []string{"crates//vendor/anyhow:anyhow"}; !reflect.DeepEqual(got, want) {
		t.Errorf("Deps = %v, want %v", got, want)
	}
}

func TestMapRustCrate(t *testing.T) {
	root := cargoWorkspaceFixture(t)
	m, err := New(testConfig(root))
	if err != nil {
		t.Fatal(err)
	}

	mapping, err := m.Language("rust").ResolveDeps(filepath.Join(root, "crates/app"), Request{})
	if err != nil {
		t.Fatal(err)
	}

	wantDeps := []string{
		// workspace member, found through its path
		"//crates/lib:lib",
		// workspace = true
		"rustdeps//vendor/anyhow:anyhow",
		// a plain version string
		"rustdeps//vendor/log:log",
		// renamed in [workspace.dependencies]: the package name
		"rustdeps//vendor/serde_json:serde_json",
		// hyphenated: the Cargo package name as rust-deps.toml has it
		"rustdeps//vendor/tree-sitter:tree-sitter",
	}
	if got := targets(mapping.Deps); !reflect.DeepEqual(got, wantDeps) {
		t.Errorf("Deps = %v, want %v", got, wantDeps)
	}
	// [dev-dependencies] go to test targets only
	if got, want := targets(mapping.TestDeps), []string{"rustdeps//vendor/tempfile:tempfile"}; !reflect.DeepEqual(got, want) {
		t.Errorf("TestDeps = %v, want %v", got, want)
	}
	if got, want := mapping.UnmappedImports, []string{"unknown-crate"}; !reflect.DeepEqual(got, want) {
		t.Errorf("UnmappedImports = %v, want %v", got, want)
	}

	// An optional dependency no feature activates isn't a dep; without a
	// platform, a target-specific table isn't evaluated
	wantUnsynced := []UnsyncedDep{
		{Dep: MappedDep{Target: "//crates/lib:lib", Type: DependencyInternal, ImportPath: "my-lib"}, Reason: "target-specific (cfg(unix))"},
	}
	if !reflect.DeepEqual(mapping.UnsyncedDeps, wantUnsynced) {
		t.Errorf("UnsyncedDeps = %+v, want %+v", mapping.UnsyncedDeps, wantUnsynced)
	}
}

// A workspace member named in a dependency without a path still maps to
// the member's target, through the members' package names.
func TestMapRustCrateMemberByName(t *testing.T) {
	root := cargoWorkspaceFixture(t)
	writeTree(t, root, map[string]string{
		"crates/other/Cargo.toml": `[package]
name = "other"

[dependencies]
my-lib = "0.1"
`,
	})
	m, err := New(testConfig(root))
	if err != nil {
		t.Fatal(err)
	}
	mapping, err := m.Language("rust").ResolveDeps(filepath.Join(root, "crates/other"), Request{})
	if err != nil {
		t.Fatal(err)
	}
	if got, want := targets(mapping.Deps), []string{"//crates/lib:lib"}; !reflect.DeepEqual(got, want) {
		t.Errorf("Deps = %v, want %v", got, want)
	}
}

// A workspace = true entry the workspace doesn't declare is an error, not
// a silently missing dep.
func TestMapRustCrateMissingWorkspaceDep(t *testing.T) {
	root := cargoWorkspaceFixture(t)
	writeTree(t, root, map[string]string{
		"crates/bad/Cargo.toml": `[package]
name = "bad"

[dependencies]
nope.workspace = true
`,
	})
	m, err := New(testConfig(root))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := m.Language("rust").ResolveDeps(filepath.Join(root, "crates/bad"), Request{}); err == nil {
		t.Error("no error for a workspace = true dep missing from [workspace.dependencies]")
	}
}

// A [target.'<spec>'.*] table applies in the configurations whose platform
// its spec holds on, like the unconditional tables.
func TestMapRustCrateTargetSpecific(t *testing.T) {
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"Cargo.toml": "[workspace]\nmembers = [\"crates/*\"]\n",
		"rust-deps.toml": `[deps."libc@0.2.0"]
name = "libc"
[deps."inotify@0.11.0"]
name = "inotify"
[deps."core-foundation@0.10.0"]
name = "core-foundation"
[deps."tempfile@3.0.0"]
name = "tempfile"
`,
		"crates/app/Cargo.toml": `[package]
name = "app"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "linux")'.dependencies]
inotify = "0.11"

[target.aarch64-apple-darwin.dependencies]
core-foundation = "0.10"

[target.'cfg(target_os = "linux")'.dev-dependencies]
tempfile = "3"
`,
	})
	m, err := New(testConfig(root))
	if err != nil {
		t.Fatal(err)
	}
	lang := m.Language("rust")
	crate := filepath.Join(root, "crates/app")

	dims, err := lang.Dimensions(crate)
	if err != nil {
		t.Fatal(err)
	}
	if want := (Dimensions{Platform: []string{"os", "cpu"}}); !reflect.DeepEqual(dims, want) {
		t.Errorf("dimensions = %v, want %v", dims, want)
	}

	for _, tc := range []struct {
		os, cpu        string
		deps, testDeps []string
	}{
		{"linux", "x86_64", []string{"rustdeps//vendor/inotify:inotify", "rustdeps//vendor/libc:libc"}, []string{"rustdeps//vendor/tempfile:tempfile"}},
		{"macos", "x86_64", []string{"rustdeps//vendor/libc:libc"}, nil},
		{"macos", "arm64", []string{"rustdeps//vendor/core-foundation:core-foundation", "rustdeps//vendor/libc:libc"}, nil},
	} {
		mapping, err := lang.ResolveDeps(crate, Request{Config: map[string]string{"os": tc.os, "cpu": tc.cpu}})
		if err != nil {
			t.Fatal(err)
		}
		if got := targets(mapping.Deps); !reflect.DeepEqual(got, tc.deps) {
			t.Errorf("%s-%s: deps = %v, want %v", tc.os, tc.cpu, got, tc.deps)
		}
		if got := targets(mapping.TestDeps); !reflect.DeepEqual(got, tc.testDeps) {
			t.Errorf("%s-%s: test deps = %v, want %v", tc.os, tc.cpu, got, tc.testDeps)
		}
		if len(mapping.UnsyncedDeps) != 0 {
			t.Errorf("%s-%s: unsynced = %+v, want none", tc.os, tc.cpu, mapping.UnsyncedDeps)
		}
	}

	// Without a platform, target-specific tables stay unsynced
	mapping, err := lang.ResolveDeps(crate, Request{})
	if err != nil {
		t.Fatal(err)
	}
	if len(mapping.Deps) != 0 || len(mapping.UnsyncedDeps) != 4 {
		t.Errorf("without a platform: deps = %v, unsynced = %+v", mapping.Deps, mapping.UnsyncedDeps)
	}

}

// featuresFixture is a workspace whose member lib has a primary target
// and a variant, and whose member app depends on lib.
func featuresFixture(t *testing.T, appDeps string) (string, Language) {
	t.Helper()
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"Cargo.toml": `[workspace]
members = ["crates/*"]

[workspace.dependencies]
lib = { path = "crates/lib", features = ["base"] }
`,
		"rust-deps.toml": `[deps."libc@0.2.0"]
name = "libc"
[deps."notify@8.0.0"]
name = "notify"
[deps."log@0.4.0"]
name = "log"
`,
		"crates/lib/Cargo.toml": `[package]
name = "lib"

[features]
default = ["base"]
base = []
watch = ["dep:notify", "log/std"]
fuse = ["dep:libc"]

[dependencies]
log = "0.4"
libc = { version = "0.2", optional = true }
notify = { version = "8", optional = true }
`,
		"crates/lib/rules.star": `rust_library(
    name = "lib",
)

rust_library(
    name = "lib-full",
    cargo_features = ["watch"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": [],
    }),
)

rust_library(
    name = "lib-bare",
    default_features = False,
)
`,
		"crates/app/Cargo.toml": "[package]\nname = \"app\"\n\n[dependencies]\n" + appDeps,
	})
	cfg := testConfig(root)
	cfg.Conditions.Platforms = []conditions.Platform{{OS: "linux", CPU: "x86_64"}, {OS: "macos", CPU: "arm64"}}
	m, err := New(cfg)
	if err != nil {
		t.Fatal(err)
	}
	return root, m.Language("rust")
}

func linux() conditions.Configuration {
	return conditions.Configuration{"os": "linux", "cpu": "x86_64"}
}
func macos() conditions.Configuration { return conditions.Configuration{"os": "macos", "cpu": "arm64"} }

// A primary target (no variant attributes) builds the crate's default
// features, expanded, and the optional dependencies they activate.
func TestRustPrimaryTargetBuildsDefaults(t *testing.T) {
	root, lang := featuresFixture(t, "")
	mapping, err := lang.ResolveDeps(filepath.Join(root, "crates/lib"), Request{Config: linux()})
	if err != nil {
		t.Fatal(err)
	}
	if got, want := mapping.Attrs["features"], []string{"base"}; !reflect.DeepEqual(got, want) {
		t.Errorf("features = %v, want %v", got, want)
	}
	if got, want := targets(mapping.Deps), []string{"rustdeps//vendor/log:log"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
}

// default_features = False leaves the defaults out of the request.
func TestRustDefaultFeaturesFalse(t *testing.T) {
	root, lang := featuresFixture(t, "")
	mapping, err := lang.ResolveDeps(filepath.Join(root, "crates/lib"), Request{
		Config:  linux(),
		Variant: map[string]starlark.AttributeValue{"default_features": starlark.BoolValue{Value: false}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got := mapping.Attrs["features"]; len(got) != 0 {
		t.Errorf("features = %v, want none", got)
	}
}

// cargo_features are expanded with the defaults: dep: activates optional
// dependencies, and x/feat forwards.
func TestRustCargoFeaturesExpand(t *testing.T) {
	root, lang := featuresFixture(t, "")
	mapping, err := lang.ResolveDeps(filepath.Join(root, "crates/lib"), Request{
		Config:  linux(),
		Variant: map[string]starlark.AttributeValue{"cargo_features": starlark.StringListValue{Values: []string{"watch", "fuse"}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got, want := mapping.Attrs["features"], []string{"base", "fuse", "watch"}; !reflect.DeepEqual(got, want) {
		t.Errorf("features = %v, want %v", got, want)
	}
	want := []string{"rustdeps//vendor/libc:libc", "rustdeps//vendor/log:log", "rustdeps//vendor/notify:notify"}
	if got := targets(mapping.Deps); !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
}

// A dependency asking for a member's features maps to the member target
// whose request enables exactly those features, per configuration: its
// cargo_features may be a select().
func TestRustDependencyMapsToVariantTarget(t *testing.T) {
	root, lang := featuresFixture(t, `lib = { workspace = true, features = ["watch"] }

[target.'cfg(target_os = "linux")'.dependencies]
lib = { workspace = true, features = ["watch", "fuse"] }
`)
	for _, config := range []conditions.Configuration{linux(), macos()} {
		mapping, err := lang.ResolveDeps(filepath.Join(root, "crates/app"), Request{Config: config})
		if err != nil {
			t.Fatal(err)
		}
		if got, want := targets(mapping.Deps), []string{"//crates/lib:lib-full"}; !reflect.DeepEqual(got, want) {
			t.Errorf("%s: deps = %v, want %v (unmapped %v)", config, got, want, mapping.UnmappedImports)
		}
	}
}

// With no target building exactly the features asked for, or several, the
// dependency is unmapped, and the report names the features.
func TestRustDependencyWithoutMatchingTarget(t *testing.T) {
	root, lang := featuresFixture(t, `lib = { workspace = true, features = ["fuse"] }`)
	mapping, err := lang.ResolveDeps(filepath.Join(root, "crates/app"), Request{Config: linux()})
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"lib (needs features base, fuse: no rust_library of //crates/lib builds exactly them)"}
	if len(mapping.Deps) != 0 || !reflect.DeepEqual(mapping.UnmappedImports, want) {
		t.Errorf("deps = %v, unmapped = %v, want %v", targets(mapping.Deps), mapping.UnmappedImports, want)
	}

	// lib and lib-bare both build base only with default-features = false
	// and features = ["base"]
	root, lang = featuresFixture(t, `lib = { path = "../lib", default-features = false, features = ["base"] }`)
	writeTree(t, root, map[string]string{"crates/lib/rules.star": "rust_library(name = \"lib\")\n\nrust_library(\n    name = \"lib-base\",\n    cargo_features = [\"base\"],\n    default_features = False,\n)\n"})
	mapping, err = lang.ResolveDeps(filepath.Join(root, "crates/app"), Request{Config: linux()})
	if err != nil {
		t.Fatal(err)
	}
	want = []string{"lib (needs features base: several rust_library targets of //crates/lib build them: lib, lib-base)"}
	if len(mapping.Deps) != 0 || !reflect.DeepEqual(mapping.UnmappedImports, want) {
		t.Errorf("deps = %v, unmapped = %v, want %v", targets(mapping.Deps), mapping.UnmappedImports, want)
	}
}
