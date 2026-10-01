package main

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

// rulesProject writes a Rust project whose one rules.star holds rulesStar,
// and returns its root.
func rulesProject(t *testing.T, rulesStar string) string {
	t.Helper()
	root := t.TempDir()
	files := map[string]string{
		".buckconfig":        "",
		".turnkey/sync.toml": "[[languages]]\nname = \"rust\"\ncell = \"rustdeps\"\ndeps_file = \"rust-deps.toml\"\n",
		"Cargo.toml":         "[workspace]\nmembers = [\"lib\"]\n",
		"lib/Cargo.toml":     "[package]\nname = \"lib\"\n",
		"lib/rules.star":     rulesStar,
	}
	for rel, content := range files {
		path := filepath.Join(root, rel)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

// A target whose deps sync can't read fails tk rules check, unless a
// "# turnkey:no-sync" comment opts it out.
func TestRulesCheckUnreadableDeps(t *testing.T) {
	const unreadable = `_DEPS = []

rust_library(
    name = "lib",
    deps = _DEPS,
)
`
	// --quiet sets the package-level flag; restore it
	t.Cleanup(func() { quiet = false })

	root := rulesProject(t, unreadable)
	if code := checkRules(root, []string{"--all", "--quiet"}); code != 1 {
		t.Errorf("unreadable deps: exit code %d, want 1", code)
	}

	root = rulesProject(t, "# turnkey:no-sync\n"+unreadable[len("_DEPS = []\n\n"):])
	if code := checkRules(root, []string{"--all", "--quiet"}); code != 0 {
		t.Errorf("opted out: exit code %d, want 0", code)
	}
}

// tk rules check checks a rules.star whose sources are older than it, as
// they are once it is committed: without --all, and in a directory git
// reports no change in.
func TestRulesCheckIgnoresFileTimes(t *testing.T) {
	t.Cleanup(func() { quiet = false })

	root := rulesProject(t, `_DEPS = []

rust_library(
    name = "lib",
    deps = _DEPS,
)
`)
	old := time.Now().Add(-time.Hour)
	if err := os.Chtimes(filepath.Join(root, "lib/Cargo.toml"), old, old); err != nil {
		t.Fatal(err)
	}
	if code := checkRules(root, []string{"--quiet"}); code != 1 {
		t.Errorf("exit code %d, want 1: the unreadable deps went unchecked", code)
	}
}
