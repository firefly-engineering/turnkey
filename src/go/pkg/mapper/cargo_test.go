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
