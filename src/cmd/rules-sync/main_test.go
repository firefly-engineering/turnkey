package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rulesreport"
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

// runReport runs rules-sync with args, and returns its exit code and the
// report it printed.
func runReport(t *testing.T, args ...string) (int, rulesreport.Report) {
	t.Helper()
	var stdout, stderr bytes.Buffer
	code := run(args, &stdout, &stderr)
	var report rulesreport.Report
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("report %q: %v (stderr: %s)", stdout.String(), err, stderr.String())
	}
	return code, report
}

const unreadable = `_DEPS = []

rust_library(
    name = "lib",
    deps = _DEPS,
)
`

// The report lists a target whose deps sync can't read as unreadable, and
// one a "# turnkey:no-sync" comment opts out as opted out.
func TestReportsUnreadableDeps(t *testing.T) {
	root := rulesProject(t, unreadable)
	code, report := runReport(t, "--project-root", root, "--dry-run", "--force")
	if code != 0 || report.Error != nil {
		t.Fatalf("exit code %d, error %+v", code, report.Error)
	}
	want := []rulesreport.Result{{
		Path:       filepath.Join(root, "lib/rules.star"),
		Unreadable: []rulesreport.UnreadableTarget{{Target: "lib", Attribute: "deps"}},
	}}
	if !resultsEqual(report.Results, want) {
		t.Errorf("results %+v, want %+v", report.Results, want)
	}

	root = rulesProject(t, "# turnkey:no-sync\n"+unreadable[len("_DEPS = []\n\n"):])
	_, report = runReport(t, "--project-root", root, "--dry-run", "--force")
	want = []rulesreport.Result{{
		Path:     filepath.Join(root, "lib/rules.star"),
		OptedOut: []string{"lib"},
	}}
	if !resultsEqual(report.Results, want) {
		t.Errorf("opted out: results %+v, want %+v", report.Results, want)
	}
}

// The directory argument limits sync to the rules.star files under it.
func TestSyncsTheDirectoryGiven(t *testing.T) {
	root := rulesProject(t, unreadable)
	if err := os.Mkdir(filepath.Join(root, "empty"), 0755); err != nil {
		t.Fatal(err)
	}
	code, report := runReport(t, "--project-root", root, "--force", filepath.Join(root, "empty"))
	if code != 0 || len(report.Results) != 0 {
		t.Errorf("exit code %d, results %+v, want none", code, report.Results)
	}
}

// An error that stops sync is in the report, with where it stopped, and
// rules-sync exits 1.
func TestReportsErrors(t *testing.T) {
	root := rulesProject(t, unreadable)
	if err := os.WriteFile(filepath.Join(root, ".turnkey/sync.toml"), nil, 0644); err != nil {
		t.Fatal(err)
	}
	code, report := runReport(t, "--project-root", root)
	if code != 1 || report.Error == nil || report.Error.Stage != rulesreport.StageSetup {
		t.Errorf("no languages: exit code %d, error %+v, want a setup error", code, report.Error)
	}

	root = rulesProject(t, unreadable)
	code, report = runReport(t, "--project-root", root, "--force", filepath.Join(root, "missing"))
	if code != 1 || report.Error == nil || report.Error.Stage != rulesreport.StageSync {
		t.Errorf("missing directory: exit code %d, error %+v, want a sync error", code, report.Error)
	}
}

// Bad usage prints no report and exits 2.
func TestUsage(t *testing.T) {
	for _, args := range [][]string{
		{},
		{"--project-root", "a", "b", "c"},
		{"--bogus"},
	} {
		var stdout, stderr bytes.Buffer
		if code := run(args, &stdout, &stderr); code != 2 || stdout.Len() != 0 {
			t.Errorf("%q: exit code %d, stdout %q, want 2 and nothing", args, code, stdout.String())
		}
	}
}

func resultsEqual(a, b []rulesreport.Result) bool {
	return slices.EqualFunc(a, b, func(x, y rulesreport.Result) bool {
		return x.Path == y.Path && x.Updated == y.Updated && x.Skipped == y.Skipped &&
			len(x.Changes) == len(y.Changes) && len(x.Errors) == len(y.Errors) &&
			slices.Equal(x.OptedOut, y.OptedOut) && slices.Equal(x.Unreadable, y.Unreadable)
	})
}
