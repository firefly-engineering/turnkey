package main

import (
	"os"
	"path/filepath"
	"testing"
)

// rulesProject writes a Rust project whose one rules.star holds rulesStar,
// and makes it the working directory.
func rulesProject(t *testing.T, rulesStar string) {
	t.Helper()
	root := t.TempDir()
	files := map[string]string{
		".buckconfig":    "",
		"Cargo.toml":     "[workspace]\nmembers = [\"lib\"]\n",
		"lib/Cargo.toml": "[package]\nname = \"lib\"\n",
		"lib/rules.star": rulesStar,
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
	t.Chdir(root)
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

	rulesProject(t, unreadable)
	if code := runRulesCheck([]string{"--all", "--quiet"}); code != 1 {
		t.Errorf("unreadable deps: exit code %d, want 1", code)
	}

	rulesProject(t, "# turnkey:no-sync\n"+unreadable[len("_DEPS = []\n\n"):])
	if code := runRulesCheck([]string{"--all", "--quiet"}); code != 0 {
		t.Errorf("opted out: exit code %d, want 0", code)
	}
}
