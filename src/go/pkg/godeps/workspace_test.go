package godeps

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// writeFiles writes files (path relative to root -> content) under root.
func writeFiles(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		path := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

// fakeLister reports a fixed build list.
type fakeLister []ListedModule

func (f fakeLister) ListModules(string, *Workspace) ([]ListedModule, error) {
	return f, nil
}

// twoMembers is a go.work workspace where member a requires member b.
var twoMembers = map[string]string{
	"go.work": "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n",
	"a/go.mod": `module example.com/a

go 1.22

require (
	example.com/b v0.0.0
	github.com/google/uuid v1.3.0
)

replace example.com/b => ../b
`,
	"b/go.mod": `module example.com/b

go 1.22

require golang.org/x/mod v0.31.0 // indirect
`,
	"b/go.sum": "golang.org/x/mod v0.31.0 h1:mod=\ngolang.org/x/mod v0.31.0/go.mod h1:gomod=\n",
}

func TestWorkspaceMembersAreRecordedAndNeverDeps(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, twoMembers)

	ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
	if err != nil {
		t.Fatal(err)
	}
	wantMembers := []string{"example.com/a=a", "example.com/b=b"}
	var members []string
	for _, m := range ws.Members {
		members = append(members, m.Path+"="+m.Dir)
	}
	if !reflect.DeepEqual(members, wantMembers) {
		t.Errorf("members %v, want %v", members, wantMembers)
	}
	wantSources := []string{"go.work", "go.work.sum", "a/go.mod", "a/go.sum", "b/go.mod", "b/go.sum"}
	if !reflect.DeepEqual(ws.Sources, wantSources) {
		t.Errorf("sources %v, want %v", ws.Sources, wantSources)
	}

	deps, err := ws.Resolve(root, fakeLister{
		{Path: "example.com/a", Main: true},
		{Path: "example.com/b", Main: true},
		{Path: "github.com/google/uuid", Version: "v1.3.0"},
		{Path: "golang.org/x/mod", Version: "v0.31.0"},
	}, DefaultParseOptions())
	if err != nil {
		t.Fatal(err)
	}
	want := []Dependency{
		{ImportPath: "github.com/google/uuid", Version: "v1.3.0"},
		{ImportPath: "golang.org/x/mod", Version: "v0.31.0", Indirect: true, GoSumHash: "h1:mod="},
	}
	if !reflect.DeepEqual(deps, want) {
		t.Errorf("deps %+v, want %+v", deps, want)
	}
}

func TestWorkspaceRecordsTheVersionGoSelects(t *testing.T) {
	// Workspace MVS can select a version above what any member names, when
	// a dependency's go.mod asks for it
	root := t.TempDir()
	writeFiles(t, root, twoMembers)
	ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
	if err != nil {
		t.Fatal(err)
	}

	deps, err := ws.Resolve(root, fakeLister{
		{Path: "example.com/a", Main: true},
		{Path: "example.com/b", Main: true},
		{Path: "github.com/google/uuid", Version: "v1.6.0"},
		{Path: "golang.org/x/mod", Version: "v0.31.0"},
	}, DefaultParseOptions())
	if err != nil {
		t.Fatal(err)
	}
	if deps[0].Version != "v1.6.0" {
		t.Errorf("uuid at %s, want the selected v1.6.0", deps[0].Version)
	}
}

func TestWorkspaceFetchesAnExternalReplacement(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"go.mod": "module example.com/m\n\nrequire github.com/up/lib v1.0.0\n\nreplace github.com/up/lib => github.com/fork/lib v1.0.1\n",
	})
	ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
	if err != nil {
		t.Fatal(err)
	}

	deps, err := ws.Resolve(root, fakeLister{
		{Path: "example.com/m", Main: true},
		{Path: "github.com/up/lib", Version: "v1.0.0", Replace: &ListedModule{Path: "github.com/fork/lib", Version: "v1.0.1"}},
	}, DefaultParseOptions())
	if err != nil {
		t.Fatal(err)
	}
	want := []Dependency{{ImportPath: "github.com/up/lib", FetchPath: "github.com/fork/lib", Version: "v1.0.1"}}
	if !reflect.DeepEqual(deps, want) {
		t.Errorf("deps %+v, want %+v", deps, want)
	}
}

func TestALocalReplaceMustBeAMember(t *testing.T) {
	for name, files := range map[string]map[string]string{
		"in a go.mod without go.work": {
			"go.mod":     "module example.com/m\n\nrequire example.com/lib v0.0.0\n\nreplace example.com/lib => ./lib\n",
			"lib/go.mod": "module example.com/lib\n",
		},
		"outside the project": {
			"go.mod": "module example.com/m\n\nrequire example.com/lib v0.0.0\n\nreplace example.com/lib => ../lib\n",
		},
		"in go.work": {
			"go.work":    "go 1.22\n\nuse ./m\n\nreplace example.com/lib => ./lib\n",
			"m/go.mod":   "module example.com/m\n",
			"lib/go.mod": "module example.com/lib\n",
		},
	} {
		t.Run(name, func(t *testing.T) {
			root := t.TempDir()
			writeFiles(t, root, files)
			ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
			if err != nil {
				t.Fatal(err)
			}
			_, err = ws.Resolve(root, fakeLister{}, DefaultParseOptions())
			if err == nil || !strings.Contains(err.Error(), "add it to go.work") {
				t.Errorf("got %v, want an error saying to add it to go.work", err)
			}
		})
	}
}

func TestASingleModuleIsAWorkspaceOfOne(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"go.mod": "module example.com/m\n\nrequire github.com/google/uuid v1.6.0\n",
		"go.sum": "github.com/google/uuid v1.6.0 h1:uuid=\n",
	})
	ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
	if err != nil {
		t.Fatal(err)
	}
	if ws.WorkFile != "" || !reflect.DeepEqual(ws.Sources, []string{"go.mod", "go.sum"}) {
		t.Errorf("work file %q, sources %v; want none and go.mod, go.sum", ws.WorkFile, ws.Sources)
	}
	if want := []Member{{Path: "example.com/m", Dir: ".", sumFile: "go.sum"}}; !reflect.DeepEqual(ws.Members, want) {
		t.Errorf("members %+v, want %+v", ws.Members, want)
	}
	deps, err := ws.Resolve(root, fakeLister{
		{Path: "example.com/m", Main: true},
		{Path: "github.com/google/uuid", Version: "v1.6.0"},
	}, DefaultParseOptions())
	if err != nil {
		t.Fatal(err)
	}
	if want := []Dependency{{ImportPath: "github.com/google/uuid", Version: "v1.6.0", GoSumHash: "h1:uuid="}}; !reflect.DeepEqual(deps, want) {
		t.Errorf("deps %+v, want %+v", deps, want)
	}
}

func TestGoListerResolvesAWorkspace(t *testing.T) {
	goBin, err := exec.LookPath("go")
	if err != nil {
		t.Skip("no go on PATH")
	}
	root := t.TempDir()
	// Members only, so go needs neither the network nor a module cache.
	// Go fetches a member's go.mod at the required version unless a
	// replace points the require at the member.
	writeFiles(t, root, map[string]string{
		"go.work":  "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n",
		"a/go.mod": "module example.com/a\n\ngo 1.22\n\nrequire example.com/b v0.0.0\n\nreplace example.com/b => ../b\n",
		"b/go.mod": "module example.com/b\n\ngo 1.22\n",
	})
	ws, err := LoadWorkspace(root, "go.work", "go.mod", "go.sum")
	if err != nil {
		t.Fatal(err)
	}
	// An inherited GOWORK=off must not turn workspace mode off
	lister := GoLister{Go: goBin, Env: append(os.Environ(), "GOWORK=off", "GOTOOLCHAIN=local")}
	listed, err := lister.ListModules(root, ws)
	if err != nil {
		t.Fatal(err)
	}
	var main []string
	for _, m := range listed {
		if m.Main {
			main = append(main, m.Path)
		}
	}
	if want := []string{"example.com/a", "example.com/b"}; !reflect.DeepEqual(main, want) {
		t.Errorf("main modules %v, want %v", main, want)
	}
}

func TestWriteDepsFileRecordsSourcesAndMembers(t *testing.T) {
	var buf bytes.Buffer
	file := DepsFile{
		Deps:    []Dependency{{ImportPath: "github.com/google/uuid", Version: "v1.6.0", NixHash: "sha256-x"}},
		Sources: []string{"go.work", "a/go.mod"},
		Members: []Member{{Path: "example.com/a", Dir: "a"}},
	}
	if err := WriteDepsFile(&buf, file, OutputOptions{}); err != nil {
		t.Fatal(err)
	}
	want := `schema_version = 2

sources = [
    "go.work",
    "a/go.mod",
]

[deps."github.com/google/uuid@v1.6.0"]
import_path = "github.com/google/uuid"
version = "v1.6.0"
hash = "sha256-x"

[members."example.com/a"]
dir = "a"

`
	if buf.String() != want {
		t.Errorf("got\n%s\nwant\n%s", buf.String(), want)
	}
}
