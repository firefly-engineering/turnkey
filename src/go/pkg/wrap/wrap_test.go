package wrap

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// fakeGo stands for the go tool in a project at root: `go get` adds a
// require line to go.mod, anything else leaves it be, and every run exits
// with exitCode. It records what it ran, one line per run.
type fakeGo struct {
	root     string
	exitCode int
	ran      []string
}

func (f *fakeGo) exec(dir, tool string, args []string) int {
	f.ran = append(f.ran, strings.TrimSpace(dir+" "+tool+" "+strings.Join(args, " ")))
	if tool == "go" && len(args) > 0 && args[0] == "get" {
		path := filepath.Join(f.root, "go.mod")
		data, _ := os.ReadFile(path)
		data = append(data, "require "+args[1]+" v1.0.0\n"...)
		if err := os.WriteFile(path, data, 0o644); err != nil {
			panic(err)
		}
	}
	return f.exitCode
}

// newWrapper returns a Wrapper for a project holding a go.mod, whose go
// wrapper rule runs `go mod tidy` and syncs the "go" rule, with the fake go
// tool and a Sync recording the rules it was asked for.
func newWrapper(t *testing.T) (*Wrapper, *fakeGo, *[]string) {
	t.Helper()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "go.mod"), []byte("module example.com/m\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	tool := &fakeGo{root: root}
	var synced []string
	w := &Wrapper{
		Root: root,
		Config: &syncconfig.Config{Wrappers: []syncconfig.WrapperRule{{
			Name:                "go",
			Command:             "go",
			MutatingSubcommands: []string{"get", "mod"},
			WatchFiles:          []string{"go.mod", "go.sum"},
			DepsRule:            "go",
			PostCommands:        []string{"go mod tidy"},
		}}},
		Exec: tool.exec,
		Sync: func(rule string) error {
			synced = append(synced, rule)
			return nil
		},
		Log: &bytes.Buffer{},
	}
	return w, tool, &synced
}

func TestRunWithoutChangeDoesNotSync(t *testing.T) {
	w, tool, synced := newWrapper(t)

	if code := w.Run("go", []string{"mod", "verify"}); code != 0 {
		t.Errorf("exit code %d, want 0", code)
	}
	if len(*synced) != 0 {
		t.Errorf("synced %v; go.mod didn't change", *synced)
	}
	if want := []string{"go mod verify"}; !reflect.DeepEqual(tool.ran, want) {
		t.Errorf("ran %q, want %q (no post-commands)", tool.ran, want)
	}
}

func TestRunWithChangeRunsPostCommandsThenSyncsTheRule(t *testing.T) {
	w, tool, synced := newWrapper(t)

	if code := w.Run("go", []string{"get", "example.com/dep"}); code != 0 {
		t.Errorf("exit code %d, want 0", code)
	}
	want := []string{"go get example.com/dep", w.Root + " go mod tidy"}
	if !reflect.DeepEqual(tool.ran, want) {
		t.Errorf("ran %q, want %q", tool.ran, want)
	}
	if want := []string{"go"}; !reflect.DeepEqual(*synced, want) {
		t.Errorf("synced %v, want %v", *synced, want)
	}
}

func TestRunPassesTheToolsExitCodeThrough(t *testing.T) {
	w, tool, synced := newWrapper(t)
	tool.exitCode = 3
	w.Sync = func(rule string) error {
		*synced = append(*synced, rule)
		return errors.New("generator failed")
	}

	// The tool changed go.mod before failing: its files are synced all the
	// same, and neither the post-command's nor the sync's failure hides the
	// tool's exit code.
	if code := w.Run("go", []string{"get", "example.com/dep"}); code != 3 {
		t.Errorf("exit code %d, want 3", code)
	}
	if len(*synced) != 1 {
		t.Errorf("synced %v, want the go rule once", *synced)
	}
	if code := w.Run("go", []string{"build", "./..."}); code != 3 {
		t.Errorf("passed-through exit code %d, want 3", code)
	}
}

func TestRunPassesThroughWithoutSyncing(t *testing.T) {
	for name, tc := range map[string]struct {
		tool   string
		noSync bool
	}{
		"no-sync":            {tool: "go", noSync: true},
		"tool without rules": {tool: "gofmt"},
	} {
		t.Run(name, func(t *testing.T) {
			w, tool, synced := newWrapper(t)
			w.NoSync = tc.noSync

			w.Run(tc.tool, []string{"get", "example.com/dep"})
			if len(*synced) != 0 {
				t.Errorf("synced %v", *synced)
			}
			if want := []string{tc.tool + " get example.com/dep"}; !reflect.DeepEqual(tool.ran, want) {
				t.Errorf("ran %q, want %q", tool.ran, want)
			}
		})
	}
}

func TestRunDoesNotWatchANonMutatingSubcommand(t *testing.T) {
	w, _, synced := newWrapper(t)
	w.Config.Wrappers[0].MutatingSubcommands = []string{"mod"}

	// get changes go.mod, but the rule doesn't say it may.
	w.Run("go", []string{"get", "example.com/dep"})
	if len(*synced) != 0 {
		t.Errorf("synced %v; get isn't a mutating subcommand", *synced)
	}
}

func TestOpenOutsideAProjectPassesThrough(t *testing.T) {
	dir := t.TempDir()
	tool := &fakeGo{root: dir, exitCode: 2}

	w := Open(dir, tool.exec, &bytes.Buffer{})
	if code := w.Run("go", []string{"get", "example.com/dep"}); code != 2 {
		t.Errorf("exit code %d, want 2", code)
	}
	if want := []string{"go get example.com/dep"}; !reflect.DeepEqual(tool.ran, want) {
		t.Errorf("ran %q, want %q", tool.ran, want)
	}
}

func TestOpenSyncsWithTheProjectsRules(t *testing.T) {
	root := t.TempDir()
	syncToml := `
[[deps]]
name = "go"
sources = ["go.mod"]
target = "go-deps.toml"
generator = ["sh", "-c", "wc -l < go.mod"]

[[wrappers]]
name = "go"
command = "go"
mutating_subcommands = ["get"]
watch_files = ["go.mod"]
deps_rule = "go"
`
	if err := os.MkdirAll(filepath.Join(root, ".turnkey"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, syncconfig.DefaultConfigPath), []byte(syncToml), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "go.mod"), []byte("module example.com/m\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	sub := filepath.Join(root, "src")
	if err := os.Mkdir(sub, 0o755); err != nil {
		t.Fatal(err)
	}
	tool := &fakeGo{root: root}

	w := Open(sub, tool.exec, &bytes.Buffer{})
	if code := w.Run("go", []string{"get", "example.com/dep"}); code != 0 {
		t.Errorf("exit code %d, want 0", code)
	}
	got, err := os.ReadFile(filepath.Join(root, "go-deps.toml"))
	if err != nil {
		t.Fatalf("go-deps.toml not generated: %v", err)
	}
	if strings.TrimSpace(string(got)) != "2" {
		t.Errorf("go-deps.toml = %q, want the line count of the changed go.mod", got)
	}
}

func TestRunWatchesTheSourcesTheDepsFileLists(t *testing.T) {
	w, tool, synced := newWrapper(t)
	// A go.work workspace: go-deps.toml lists a member's go.mod, which
	// `go get` in that member changes
	w.Config.Deps = []syncconfig.DepsRule{{
		Name:          "go",
		Sources:       []string{"go.work"},
		Target:        "go-deps.toml",
		TargetSources: "sources",
	}}
	w.Config.Wrappers[0].WatchFiles = []string{"go.work"}
	if err := os.WriteFile(filepath.Join(w.Root, "go-deps.toml"), []byte("sources = [\"go.mod\"]\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	w.Run("go", []string{"get", "example.com/dep"})
	if want := []string{"go"}; !reflect.DeepEqual(*synced, want) {
		t.Errorf("synced %v, want %v: go.mod is listed in go-deps.toml (ran %q)", *synced, want, tool.ran)
	}
}
