package godeps

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"golang.org/x/mod/modfile"
)

// A Workspace is the Go modules a project resolves together (ADR 0007): the
// members of its go.work, or, without one, the root go.mod alone. Paths are
// relative to the project root, with forward slashes.
type Workspace struct {
	// WorkFile is the go.work, or empty for a workspace of one go.mod.
	WorkFile string

	// Members are the workspace's modules, in go.work's order.
	Members []Member

	// Sources are the files the workspace was read from, existing or not:
	// go.work and go.work.sum, then each member's go.mod and go.sum.
	Sources []string

	// modFiles holds each member's parsed go.mod, by member index.
	modFiles []*modfile.File
	// work is the parsed go.work, nil without one.
	work *modfile.WorkFile
}

// A Member is a module of the workspace: first-party code, never fetched.
type Member struct {
	// Path is the module path its go.mod declares.
	Path string
	// Dir is its directory, relative to the project root ("." for the root).
	Dir string
	// sumFile is its go.sum (or the one named on the command line).
	sumFile string
}

// LoadWorkspace reads the workspace of the project at root. With workFile
// present it is a go.work workspace, and modFile and sumFile are unused;
// otherwise modFile (with sumFile) is its only member. File arguments are
// relative to root.
func LoadWorkspace(root, workFile, modFile, sumFile string) (*Workspace, error) {
	workData, err := os.ReadFile(filepath.Join(root, workFile))
	if errors.Is(err, os.ErrNotExist) {
		ws := &Workspace{Sources: []string{clean(modFile), clean(sumFile)}}
		dir := path.Dir(clean(modFile))
		if err := ws.addMember(root, dir, clean(modFile), clean(sumFile)); err != nil {
			return nil, err
		}
		return ws, nil
	}
	if err != nil {
		return nil, err
	}

	work, err := modfile.ParseWork(workFile, workData, nil)
	if err != nil {
		return nil, err
	}
	ws := &Workspace{
		WorkFile: clean(workFile),
		Sources:  []string{clean(workFile), clean(workFile) + ".sum"},
		work:     work,
	}
	for _, use := range work.Use {
		dir, err := projectPath(root, path.Dir(ws.WorkFile), use.Path)
		if err != nil {
			return nil, fmt.Errorf("%s: use %s: %w", workFile, use.Path, err)
		}
		mod, sum := path.Join(dir, "go.mod"), path.Join(dir, "go.sum")
		ws.Sources = append(ws.Sources, mod, sum)
		if err := ws.addMember(root, dir, mod, sum); err != nil {
			return nil, err
		}
	}
	return ws, nil
}

// addMember reads the go.mod of the member in dir.
func (ws *Workspace) addMember(root, dir, modFile, sumFile string) error {
	data, err := os.ReadFile(filepath.Join(root, modFile))
	if err != nil {
		return err
	}
	f, err := modfile.Parse(modFile, data, nil)
	if err != nil {
		return err
	}
	if f.Module == nil {
		return fmt.Errorf("%s: no module directive", modFile)
	}
	ws.Members = append(ws.Members, Member{Path: f.Module.Mod.Path, Dir: dir, sumFile: sumFile})
	ws.modFiles = append(ws.modFiles, f)
	return nil
}

// projectPath resolves p, written in the file in dir (both relative to
// root), to a clean path relative to root. A path outside the project has
// no Buck2 target, so it is an error.
func projectPath(root, dir, p string) (string, error) {
	abs := p
	if !filepath.IsAbs(p) {
		abs = filepath.Join(root, filepath.FromSlash(dir), filepath.FromSlash(p))
	}
	rel, err := filepath.Rel(root, abs)
	if err != nil {
		return "", err
	}
	rel = filepath.ToSlash(rel)
	if rel == ".." || strings.HasPrefix(rel, "../") {
		return "", fmt.Errorf("%s is outside the project", p)
	}
	return rel, nil
}

func clean(p string) string {
	return path.Clean(filepath.ToSlash(p))
}

// CheckLocalReplaces fails on a local-path replace, in go.work or a
// member's go.mod, that points anywhere but a workspace member: a local
// module is first-party code, and the members are all of it.
func (ws *Workspace) CheckLocalReplaces(root string) error {
	members := make(map[string]bool, len(ws.Members))
	for _, m := range ws.Members {
		members[m.Dir] = true
	}
	check := func(file, dir string, replaces []*modfile.Replace) error {
		for _, r := range replaces {
			if !modfile.IsDirectoryPath(r.New.Path) {
				continue
			}
			target, err := projectPath(root, dir, r.New.Path)
			if err != nil || !members[target] {
				return fmt.Errorf("%s: replace %s => %s: not a workspace member; add it to go.work", file, r.Old.Path, r.New.Path)
			}
		}
		return nil
	}
	if ws.work != nil {
		if err := check(ws.WorkFile, path.Dir(ws.WorkFile), ws.work.Replace); err != nil {
			return err
		}
	}
	for i, m := range ws.Members {
		if err := check(path.Join(m.Dir, "go.mod"), m.Dir, ws.modFiles[i].Replace); err != nil {
			return err
		}
	}
	return nil
}

// Requires returns the modules the members require, minus the members
// themselves, at the version a member names (the highest when several do).
// A module is indirect when every member requiring it says so.
func (ws *Workspace) Requires(opts ParseOptions) []Dependency {
	members := make(map[string]bool, len(ws.Members))
	for _, m := range ws.Members {
		members[m.Path] = true
	}
	byPath := make(map[string]*Dependency)
	for _, f := range ws.modFiles {
		for _, req := range f.Require {
			if members[req.Mod.Path] || (req.Indirect && !opts.IncludeIndirect) {
				continue
			}
			dep, ok := byPath[req.Mod.Path]
			if !ok {
				byPath[req.Mod.Path] = &Dependency{ImportPath: req.Mod.Path, Version: req.Mod.Version, Indirect: req.Indirect}
				continue
			}
			dep.Indirect = dep.Indirect && req.Indirect
		}
	}
	deps := make([]Dependency, 0, len(byPath))
	for _, dep := range byPath {
		deps = append(deps, *dep)
	}
	sort.Slice(deps, func(i, j int) bool { return deps[i].ImportPath < deps[j].ImportPath })
	return deps
}

// SumHashes returns the go.sum hashes of every member and of go.work.sum,
// keyed as ParseGoSum keys them. A missing go.sum has none.
func (ws *Workspace) SumHashes(root string) (map[string]string, error) {
	files := make([]string, 0, len(ws.Members)+1)
	for _, m := range ws.Members {
		files = append(files, m.sumFile)
	}
	if ws.WorkFile != "" {
		files = append(files, ws.WorkFile+".sum")
	}
	hashes := make(map[string]string)
	for _, file := range files {
		data, err := os.ReadFile(filepath.Join(root, file))
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, err
		}
		parsed, err := ParseGoSum(data)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", file, err)
		}
		maps.Copy(hashes, parsed)
	}
	return hashes, nil
}

// A ListedModule is a module of the build list Go selects, as
// `go list -m -json all` reports it.
type ListedModule struct {
	Path    string
	Version string
	// Main is true for a workspace member.
	Main    bool
	Replace *ListedModule
}

// A ModuleLister reports the build list Go selects for a workspace.
type ModuleLister interface {
	ListModules(root string, ws *Workspace) ([]ListedModule, error)
}

// GoLister lists modules with `go list -m -json all`, in workspace mode for
// a go.work workspace and in module mode otherwise.
type GoLister struct {
	// Go is the go command to run.
	Go string
	// Env is the environment go runs in, minus the GOWORK and GOFLAGS the
	// lister sets itself.
	Env []string
}

// ListModules runs go in the project at root.
func (l GoLister) ListModules(root string, ws *Workspace) ([]ListedModule, error) {
	cmd := exec.Command(l.Go, "list", "-m", "-json", "all")
	gowork := "off"
	if ws.WorkFile != "" {
		abs, err := filepath.Abs(filepath.Join(root, ws.WorkFile))
		if err != nil {
			return nil, err
		}
		gowork = abs
		cmd.Dir = filepath.Dir(abs)
	} else {
		cmd.Dir = filepath.Join(root, filepath.FromSlash(ws.Members[0].Dir))
	}
	for _, kv := range l.Env {
		if !strings.HasPrefix(kv, "GOWORK=") && !strings.HasPrefix(kv, "GOFLAGS=") {
			cmd.Env = append(cmd.Env, kv)
		}
	}
	// -mod=readonly: an untidy go.mod is an error to fix, not a file
	// for godeps-gen to rewrite
	cmd.Env = append(cmd.Env, "GOWORK="+gowork, "GOFLAGS=-mod=readonly")
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("go list -m -json all: %w: %s", err, strings.TrimSpace(stderr.String()))
	}
	var modules []ListedModule
	dec := json.NewDecoder(bytes.NewReader(out))
	for {
		var m ListedModule
		if err := dec.Decode(&m); err == io.EOF {
			break
		} else if err != nil {
			return nil, fmt.Errorf("go list -m -json all: %w", err)
		}
		modules = append(modules, m)
	}
	return modules, nil
}

// Resolve returns the workspace's dependencies: the modules its members
// require, at the version Go selects for the whole workspace (which can be
// above any version a member names), fetched from the replacement a
// non-local replace names. go.sum hashes are merged in.
func (ws *Workspace) Resolve(root string, lister ModuleLister, opts ParseOptions) ([]Dependency, error) {
	if err := ws.CheckLocalReplaces(root); err != nil {
		return nil, err
	}
	listed, err := lister.ListModules(root, ws)
	if err != nil {
		return nil, err
	}
	selected := make(map[string]ListedModule, len(listed))
	for _, m := range listed {
		selected[m.Path] = m
	}
	hashes, err := ws.SumHashes(root)
	if err != nil {
		return nil, err
	}

	var deps []Dependency
	for _, dep := range ws.Requires(opts) {
		m, ok := selected[dep.ImportPath]
		switch {
		case !ok:
			return nil, fmt.Errorf("go list -m all does not list %s, which a workspace member requires", dep.ImportPath)
		case m.Main:
			continue
		case m.Replace != nil && m.Replace.Version == "":
			// CheckLocalReplaces let it through, so Go would have made
			// the replacement a member
			return nil, fmt.Errorf("%s is replaced by local directory %s, which is not a workspace member; add it to go.work", m.Path, m.Replace.Path)
		case m.Replace != nil:
			dep.Version = m.Replace.Version
			if m.Replace.Path != dep.ImportPath {
				dep.FetchPath = m.Replace.Path
			}
		default:
			dep.Version = m.Version
		}
		dep.GoSumHash = hashes[dep.EffectiveFetchPath()+" "+dep.Version]
		deps = append(deps, dep)
	}
	return deps, nil
}
