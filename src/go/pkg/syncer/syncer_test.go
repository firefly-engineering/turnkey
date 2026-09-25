package syncer

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// newSyncer returns a Syncer over a fresh root holding one source file per
// rule, with each rule's generator printing the rule's name.
func newSyncer(t *testing.T, names ...string) (*Syncer, string) {
	t.Helper()
	root := t.TempDir()
	cfg := &syncconfig.Config{}
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(root, name+".src"), []byte(name), 0o644); err != nil {
			t.Fatal(err)
		}
		cfg.Deps = append(cfg.Deps, syncconfig.DepsRule{
			Name:      name,
			Sources:   []string{name + ".src"},
			Target:    name + ".out",
			Generator: []string{"echo", name},
		})
	}
	s := New(cfg, root)
	s.Output = &bytes.Buffer{}
	return s, root
}

func TestSyncDepsOnlySyncsNamedRules(t *testing.T) {
	s, root := newSyncer(t, "go", "python")
	s.Only = []string{"python"}

	result, err := s.SyncDeps()
	if err != nil {
		t.Fatal(err)
	}
	if result.Synced != 1 {
		t.Fatalf("synced %d rules, want 1", result.Synced)
	}
	if _, err := os.Stat(filepath.Join(root, "go.out")); !os.IsNotExist(err) {
		t.Errorf("go.out exists; only python was named")
	}
	if got, _ := os.ReadFile(filepath.Join(root, "python.out")); string(got) != "python\n" {
		t.Errorf("python.out = %q, want %q", got, "python\n")
	}
}

func TestSyncDepsRejectsUnknownRule(t *testing.T) {
	s, _ := newSyncer(t, "go")
	s.Only = []string{"haskell"}

	if _, err := s.SyncDeps(); err == nil {
		t.Fatal("SyncDeps with an unknown rule name succeeded")
	}
	if _, _, err := s.Check(); err == nil {
		t.Fatal("Check with an unknown rule name succeeded")
	}
}

func TestSyncDepsRunsRulesInConfigOrder(t *testing.T) {
	s, root := newSyncer(t, "pylock", "python")
	// python reads what pylock writes: pylock must run first even when
	// named second.
	s.Config.Deps[1].Sources = []string{"pylock.out"}
	s.Only = []string{"python", "pylock"}

	if _, err := s.SyncDeps(); err != nil {
		t.Fatal(err)
	}
	for _, out := range []string{"pylock.out", "python.out"} {
		if _, err := os.Stat(filepath.Join(root, out)); err != nil {
			t.Errorf("%s not generated: %v", out, err)
		}
	}
}

func TestARuleWithoutSourcesIsSkipped(t *testing.T) {
	// Go enabled in a project with no go.mod: nothing to generate from.
	s, root := newSyncer(t, "go")
	if err := os.Remove(filepath.Join(root, "go.src")); err != nil {
		t.Fatal(err)
	}

	result, err := s.SyncDeps()
	if err != nil || len(result.Errors) > 0 || result.Synced != 0 {
		t.Fatalf("SyncDeps = %+v, %v; want nothing synced and no error", result, err)
	}
	if _, stale, err := s.Check(); err != nil || stale {
		t.Fatalf("Check: stale %v, %v; want fresh", stale, err)
	}
}

func TestAMissingSourceDoesNotKeepARuleStale(t *testing.T) {
	// A module without dependencies has a go.mod and no go.sum.
	s, root := newSyncer(t, "go")
	s.Config.Deps[0].Sources = []string{"go.src", "go.sum"}
	if _, err := s.SyncDeps(); err != nil {
		t.Fatal(err)
	}

	if _, stale, err := s.Check(); err != nil || stale {
		t.Fatalf("Check after sync: stale %v, %v; want fresh", stale, err)
	}
	_ = root
}
