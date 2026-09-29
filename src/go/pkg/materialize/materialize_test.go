package materialize

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// fixture is a project root and a fake store, where each package's
// "store path" is a directory
type fixture struct {
	t     *testing.T
	root  string
	store string
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	dir := t.TempDir()
	f := &fixture{t: t, root: filepath.Join(dir, "project"), store: filepath.Join(dir, "store")}
	for _, d := range []string{f.root, f.store} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	return f
}

func (f *fixture) storePath(name string) string {
	p := filepath.Join(f.store, name)
	if err := os.MkdirAll(p, 0o755); err != nil {
		f.t.Fatal(err)
	}
	return p
}

// index writes a cell index for anyhow at version, and prost-derive
func (f *fixture) index(anyhowVersion string) string {
	f.t.Helper()
	anyhow := "vendor/anyhow@" + anyhowVersion
	index := Index{
		Cell:           "rustdeps",
		DepsFileSHA256: "sha-" + anyhowVersion,
		Buckconfig:     "[cells]\n    rustdeps = .\n",
		Packages: map[string]Package{
			anyhow:                       {Store: f.storePath("aaa-dep-rust-anyhow-" + anyhowVersion), Targets: []string{"anyhow"}},
			"vendor/prost-derive@0.14.4": {Store: f.storePath("bbb-dep-rust-prost-derive-0.14.4-" + anyhowVersion), Targets: []string{"prost-derive"}},
		},
		Aliases: map[string]string{"vendor/anyhow": anyhow},
	}
	data, err := json.Marshal(index)
	if err != nil {
		f.t.Fatal(err)
	}
	path := filepath.Join(f.store, "index-"+anyhowVersion+".json")
	if err := os.WriteFile(path, data, 0o644); err != nil {
		f.t.Fatal(err)
	}
	return path
}

func (f *fixture) materialize(index string) Result {
	f.t.Helper()
	result, err := Materialize(Options{Root: f.root, IndexPath: index})
	if err != nil {
		f.t.Fatal(err)
	}
	return result
}

func (f *fixture) cell(path string) string {
	return filepath.Join(CellDir(f.root, "rustdeps"), path)
}

func (f *fixture) read(path string) string {
	f.t.Helper()
	data, err := os.ReadFile(f.cell(path))
	if err != nil {
		f.t.Fatal(err)
	}
	return string(data)
}

func TestMaterializesTheIndex(t *testing.T) {
	f := newFixture(t)
	result := f.materialize(f.index("1.0.100"))

	if result.StoreLinksAdded != 2 || result.PackagesWritten != 3 {
		t.Errorf("result %+v: want 2 store links and 3 packages", result)
	}
	link := f.cell("_store/aaa-dep-rust-anyhow-1.0.100")
	if target, err := os.Readlink(link); err != nil || target != filepath.Join(f.store, "aaa-dep-rust-anyhow-1.0.100") {
		t.Errorf("store link %s -> %q, %v", link, target, err)
	}
	want := `alias(name = "anyhow", actual = "//_store/aaa-dep-rust-anyhow-1.0.100:anyhow", visibility = ["PUBLIC"])`
	for _, pkg := range []string{"vendor/anyhow@1.0.100", "vendor/anyhow"} {
		if got := f.read(pkg + "/rules.star"); !strings.Contains(got, want) {
			t.Errorf("%s/rules.star:\n%s\nwant %s", pkg, got, want)
		}
	}
	if got := f.read(".buckconfig"); got != "[cells]\n    rustdeps = .\n" {
		t.Errorf(".buckconfig = %q", got)
	}
	if got := f.read(".deps-file-sha256"); got != "sha-1.0.100\n" {
		t.Errorf("marker = %q", got)
	}
}

func TestANoOpMaterializationTouchesNothing(t *testing.T) {
	f := newFixture(t)
	index := f.index("1.0.100")
	f.materialize(index)
	past := time.Now().Add(-time.Hour)
	alias := f.cell("vendor/anyhow@1.0.100/rules.star")
	if err := os.Chtimes(alias, past, past); err != nil {
		t.Fatal(err)
	}

	if result := f.materialize(index); result.Changed() {
		t.Errorf("second materialization changed %+v", result)
	}
	if info, err := os.Stat(alias); err != nil || !info.ModTime().Equal(past) {
		t.Errorf("unchanged alias file was rewritten")
	}
}

func TestABumpSwapsOneStoreLink(t *testing.T) {
	f := newFixture(t)
	f.materialize(f.index("1.0.100"))
	result := f.materialize(f.index("1.0.101"))

	// anyhow's and prost-derive's store paths changed
	if result.StoreLinksAdded != 2 || result.StoreLinksGone != 2 {
		t.Errorf("result %+v: want 2 links added and 2 gone", result)
	}
	if _, err := os.Lstat(f.cell("vendor/anyhow@1.0.100")); !os.IsNotExist(err) {
		t.Errorf("the removed version's package is still there: %v", err)
	}
	if got := f.read("vendor/anyhow/rules.star"); !strings.Contains(got, "anyhow-1.0.101:anyhow") {
		t.Errorf("unversioned alias not rewritten:\n%s", got)
	}
}

func TestAStoreLinkIsNeverRetargeted(t *testing.T) {
	f := newFixture(t)
	index := f.index("1.0.100")
	f.materialize(index)
	link := f.cell("_store/aaa-dep-rust-anyhow-1.0.100")
	if err := os.Remove(link); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(f.storePath("elsewhere"), link); err != nil {
		t.Fatal(err)
	}

	_, err := Materialize(Options{Root: f.root, IndexPath: index})
	if err == nil || !strings.Contains(err.Error(), "never retargeted") {
		t.Fatalf("materializing over a retargeted store link: %v", err)
	}
	if target, _ := os.Readlink(link); target != filepath.Join(f.store, "elsewhere") {
		t.Errorf("the materializer retargeted the link to %s", target)
	}
}

func TestReRunningRepairsAPartialMaterialization(t *testing.T) {
	f := newFixture(t)
	index := f.index("1.0.100")
	f.materialize(index)
	// As a crash would leave it: a store link and an alias file missing,
	// and a temporary file behind
	for _, p := range []string{"_store/bbb-dep-rust-prost-derive-0.14.4-1.0.100", "vendor/anyhow/rules.star"} {
		if err := os.Remove(f.cell(p)); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(f.cell("vendor/anyhow/.tmp-rules.star-123"), nil, 0o644); err != nil {
		t.Fatal(err)
	}

	result := f.materialize(index)
	if result.StoreLinksAdded != 1 || result.PackagesWritten != 1 {
		t.Errorf("repair %+v: want 1 store link and 1 package", result)
	}
	if _, err := os.Stat(f.cell("vendor/anyhow/.tmp-rules.star-123")); !os.IsNotExist(err) {
		t.Errorf("the temporary file is still there")
	}
	if result := f.materialize(index); result.Changed() {
		t.Errorf("after the repair, a re-run changed %+v", result)
	}
}

func TestTheFirstRunReplacesTheCellSymlink(t *testing.T) {
	f := newFixture(t)
	if err := os.MkdirAll(filepath.Join(f.root, ".turnkey"), 0o755); err != nil {
		t.Fatal(err)
	}
	old := f.storePath("old-rustdeps-cell")
	if err := os.Symlink(old, CellDir(f.root, "rustdeps")); err != nil {
		t.Fatal(err)
	}

	result := f.materialize(f.index("1.0.100"))
	if !result.SwitchedOver {
		t.Errorf("result %+v: want a switch-over", result)
	}
	if info, err := os.Lstat(CellDir(f.root, "rustdeps")); err != nil || !info.IsDir() {
		t.Errorf("the cell is not a real directory: %v", err)
	}
	if _, err := os.Stat(old); err != nil {
		t.Errorf("the old cell's store path was touched: %v", err)
	}
}

func TestRootsTheIndexOnlyWhenItChanges(t *testing.T) {
	f := newFixture(t)
	var rooted []string
	addRoot := func(link, store string) error {
		rooted = append(rooted, store)
		_ = os.Remove(link)
		return os.Symlink(store, link)
	}
	index := f.index("1.0.100")
	for range 2 {
		if _, err := Materialize(Options{Root: f.root, IndexPath: index, AddRoot: addRoot}); err != nil {
			t.Fatal(err)
		}
	}
	if len(rooted) != 1 {
		t.Errorf("rooted %d times, want once", len(rooted))
	}
	if target, _ := os.Readlink(filepath.Join(f.root, ".turnkey/gcroots/rustdeps")); target != index {
		t.Errorf("gc root -> %s, want %s", target, index)
	}
}

func TestStaleComparesTheDepsFileWithTheMarker(t *testing.T) {
	f := newFixture(t)
	content := []byte("schema_version = 2\n")
	if err := os.WriteFile(filepath.Join(f.root, "rust-deps.toml"), content, 0o644); err != nil {
		t.Fatal(err)
	}
	if stale, err := Stale(f.root, "rustdeps", "rust-deps.toml"); err != nil || stale {
		t.Errorf("a cell never materialized: stale %v, %v", stale, err)
	}

	sum := sha256.Sum256(content)
	index := f.index("1.0.100")
	data, _ := os.ReadFile(index)
	var doc Index
	_ = json.Unmarshal(data, &doc)
	doc.DepsFileSHA256 = hex.EncodeToString(sum[:])
	data, _ = json.Marshal(doc)
	_ = os.WriteFile(index, data, 0o644)
	f.materialize(index)

	if stale, err := Stale(f.root, "rustdeps", "rust-deps.toml"); err != nil || stale {
		t.Errorf("matching deps file: stale %v, %v", stale, err)
	}
	if err := os.WriteFile(filepath.Join(f.root, "rust-deps.toml"), []byte("schema_version = 3\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if stale, err := Stale(f.root, "rustdeps", "rust-deps.toml"); err != nil || !stale {
		t.Errorf("changed deps file: stale %v, %v", stale, err)
	}
}
