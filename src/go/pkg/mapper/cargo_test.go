package mapper

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
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

func TestMapRustCrate(t *testing.T) {
	root := cargoWorkspaceFixture(t)
	m, err := New(Config{ProjectRoot: root})
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

	wantUnsynced := []UnsyncedDep{
		{Dep: MappedDep{Target: "rustdeps//vendor/libc:libc", Type: DependencyExternal, ImportPath: "libc"}, Reason: "optional"},
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
	m, err := New(Config{ProjectRoot: root})
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
	m, err := New(Config{ProjectRoot: root})
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
	m, err := New(Config{ProjectRoot: root})
	if err != nil {
		t.Fatal(err)
	}
	lang := m.Language("rust")
	crate := filepath.Join(root, "crates/app")

	dims, err := lang.Dimensions(crate)
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"os", "cpu"}; !reflect.DeepEqual(dims, want) {
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

	// A crate without target tables doesn't depend on the platform
	if dims, _ := lang.Dimensions(filepath.Join(cargoWorkspaceFixture(t), "crates/lib")); dims != nil {
		t.Errorf("dimensions without target tables = %v, want none", dims)
	}
}

// A dependency on a member that asks for features is unmapped: the
// member's primary target may not build them.
func TestMapRustMemberWithFeatures(t *testing.T) {
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"Cargo.toml": `[workspace]
members = ["crates/*"]

[workspace.dependencies]
lib = { path = "crates/lib", features = ["base"] }
`,
		"crates/lib/Cargo.toml": "[package]\nname = \"lib\"\n\n[features]\nbase = []\nextra = []\n",
		"crates/app/Cargo.toml": `[package]
name = "app"

[dependencies]
lib = { workspace = true, features = ["extra"] }
`,
	})
	m, err := New(Config{ProjectRoot: root})
	if err != nil {
		t.Fatal(err)
	}
	mapping, err := m.Language("rust").ResolveDeps(filepath.Join(root, "crates/app"), Request{})
	if err != nil {
		t.Fatal(err)
	}
	if len(mapping.Deps) != 0 {
		t.Errorf("deps = %v, want none", targets(mapping.Deps))
	}
	if want := []string{"lib (asks for features base, extra)"}; !reflect.DeepEqual(mapping.UnmappedImports, want) {
		t.Errorf("unmapped = %v, want %v", mapping.UnmappedImports, want)
	}
}
