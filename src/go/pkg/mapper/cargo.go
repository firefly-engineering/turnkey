package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// UnsyncedDep is a dependency a manifest declares that sync doesn't manage:
// it is neither added to nor removed from a target.
type UnsyncedDep struct {
	// Dep is the target the dependency maps to.
	Dep MappedDep

	// Reason says why it isn't synced, e.g. "optional".
	Reason string
}

// cargoManifest is the part of a Cargo.toml that sync reads.
type cargoManifest struct {
	Package *struct {
		Name string `toml:"name"`
	} `toml:"package"`
	Workspace *struct {
		Members      []string       `toml:"members"`
		Dependencies map[string]any `toml:"dependencies"`
	} `toml:"workspace"`
	Dependencies      map[string]any `toml:"dependencies"`
	DevDependencies   map[string]any `toml:"dev-dependencies"`
	BuildDependencies map[string]any `toml:"build-dependencies"`
	Target            map[string]struct {
		Dependencies      map[string]any `toml:"dependencies"`
		DevDependencies   map[string]any `toml:"dev-dependencies"`
		BuildDependencies map[string]any `toml:"build-dependencies"`
	} `toml:"target"`
}

// readCargoManifest parses the Cargo.toml in dir.
func readCargoManifest(dir string) (*cargoManifest, error) {
	content, err := os.ReadFile(filepath.Join(dir, "Cargo.toml"))
	if err != nil {
		return nil, err
	}
	var manifest cargoManifest
	if err := toml.Unmarshal(content, &manifest); err != nil {
		return nil, fmt.Errorf("parsing %s: %w", filepath.Join(dir, "Cargo.toml"), err)
	}
	return &manifest, nil
}

// cargoDep is one dependency entry of a manifest, with a workspace = true
// entry resolved against the workspace's [workspace.dependencies].
type cargoDep struct {
	// Package is the Cargo package name: the entry's package key if it
	// renames the dependency, else the entry's key.
	Package string

	// Path is the absolute directory of a path dependency, or "".
	Path string

	// Optional is set for optional = true.
	Optional bool
}

// parseCargoDep reads one dependency entry, whose value is a version string
// or a table. baseDir resolves a relative path.
func parseCargoDep(key string, value any, baseDir string) cargoDep {
	dep := cargoDep{Package: key}
	table, ok := value.(map[string]any)
	if !ok {
		return dep
	}
	if pkg, ok := table["package"].(string); ok && pkg != "" {
		dep.Package = pkg
	}
	if path, ok := table["path"].(string); ok && path != "" {
		dep.Path = filepath.Join(baseDir, path)
	}
	if optional, ok := table["optional"].(bool); ok {
		dep.Optional = optional
	}
	return dep
}

// isWorkspaceDep reports whether a dependency entry is workspace = true.
func isWorkspaceDep(value any) bool {
	table, ok := value.(map[string]any)
	if !ok {
		return false
	}
	inherited, _ := table["workspace"].(bool)
	return inherited
}

// cargoWorkspace is a Cargo workspace root: its [workspace.dependencies],
// which workspace = true entries inherit from.
type cargoWorkspace struct {
	dir  string
	deps map[string]any
}

// resolveCargoDeps reads a dependency table, resolving workspace = true
// entries against ws. It returns the entries sorted by key.
func resolveCargoDeps(table map[string]any, crateDir string, ws *cargoWorkspace) ([]cargoDep, error) {
	keys := make([]string, 0, len(table))
	for key := range table {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	deps := make([]cargoDep, 0, len(keys))
	for _, key := range keys {
		value := table[key]
		if !isWorkspaceDep(value) {
			deps = append(deps, parseCargoDep(key, value, crateDir))
			continue
		}
		if ws == nil {
			return nil, fmt.Errorf("dependency %s: workspace = true, but no workspace root found", key)
		}
		inherited, ok := ws.deps[key]
		if !ok {
			return nil, fmt.Errorf("dependency %s: workspace = true, but [workspace.dependencies] has no %s", key, key)
		}
		dep := parseCargoDep(key, inherited, ws.dir)
		// optional is set on the member's entry, never the workspace's
		dep.Optional = parseCargoDep(key, value, crateDir).Optional
		deps = append(deps, dep)
	}
	return deps, nil
}

// loadCargoWorkspace reads the workspace root's manifest in dir: its
// [workspace.dependencies], and each member's package name, keyed to the
// member's directory relative to projectRoot.
func loadCargoWorkspace(dir, projectRoot string) (*cargoWorkspace, map[string]string, error) {
	manifest, err := readCargoManifest(dir)
	if err != nil {
		return nil, nil, err
	}
	if manifest.Workspace == nil {
		return nil, nil, fmt.Errorf("%s has no [workspace]", filepath.Join(dir, "Cargo.toml"))
	}

	ws := &cargoWorkspace{dir: dir, deps: manifest.Workspace.Dependencies}
	members := make(map[string]string)
	for _, pattern := range manifest.Workspace.Members {
		dirs, err := filepath.Glob(filepath.Join(dir, pattern))
		if err != nil {
			return nil, nil, fmt.Errorf("workspace member %q: %w", pattern, err)
		}
		for _, memberDir := range dirs {
			member, err := readCargoManifest(memberDir)
			if err != nil || member.Package == nil {
				continue
			}
			rel, err := filepath.Rel(projectRoot, memberDir)
			if err != nil {
				continue
			}
			members[member.Package.Name] = filepath.ToSlash(rel)
		}
	}
	return ws, members, nil
}

// MapRustCrate resolves a Rust crate's deps from its Cargo.toml:
// [dependencies] become Deps, [dev-dependencies] TestDeps. Workspace members
// map to their Buck2 target, other crates to the external cell. A crate
// neither is reported in UnmappedImports (or UnmappedTestImports).
// Optional, target-specific and build dependencies are reported in
// UnsyncedDeps.
func (m *Mapper) MapRustCrate(crateDir string) (PackageMapping, error) {
	rel, err := filepath.Rel(m.config.ProjectRoot, crateDir)
	if err != nil {
		rel = crateDir
	}
	mapping := PackageMapping{Path: filepath.ToSlash(rel)}

	cfg := m.config.Rust
	if cfg == nil {
		return mapping, fmt.Errorf("no Rust configuration: the project has no Cargo.toml")
	}
	manifest, err := readCargoManifest(crateDir)
	if err != nil {
		return mapping, err
	}

	// A crate with its own [workspace] is its own root; any other inherits
	// from the project's.
	ws := cfg.workspace
	if manifest.Workspace != nil {
		ws = &cargoWorkspace{dir: crateDir, deps: manifest.Workspace.Dependencies}
	}

	resolve := func(table map[string]any) ([]cargoDep, error) {
		return resolveCargoDeps(table, crateDir, ws)
	}

	deps, err := resolve(manifest.Dependencies)
	if err != nil {
		return mapping, err
	}
	for _, dep := range deps {
		mapped, ok := m.mapCargoDep(dep)
		switch {
		case !ok:
			mapping.UnmappedImports = append(mapping.UnmappedImports, dep.Package)
		case dep.Optional:
			mapping.UnsyncedDeps = append(mapping.UnsyncedDeps, UnsyncedDep{Dep: mapped, Reason: "optional"})
		default:
			mapping.Deps = append(mapping.Deps, mapped)
		}
	}

	devDeps, err := resolve(manifest.DevDependencies)
	if err != nil {
		return mapping, err
	}
	for _, dep := range devDeps {
		if mapped, ok := m.mapCargoDep(dep); ok {
			mapping.TestDeps = append(mapping.TestDeps, mapped)
		} else {
			mapping.UnmappedTestImports = append(mapping.UnmappedTestImports, dep.Package)
		}
	}

	// Sync doesn't manage these, but reports each so none is dropped
	// silently.
	unsynced := func(table map[string]any, reason string) error {
		deps, err := resolve(table)
		if err != nil {
			return err
		}
		for _, dep := range deps {
			mapped, ok := m.mapCargoDep(dep)
			if !ok {
				mapped = MappedDep{Type: DependencyUnmapped, ImportPath: dep.Package}
			}
			mapping.UnsyncedDeps = append(mapping.UnsyncedDeps, UnsyncedDep{Dep: mapped, Reason: reason})
		}
		return nil
	}
	if err := unsynced(manifest.BuildDependencies, "build dependency"); err != nil {
		return mapping, err
	}
	platforms := make([]string, 0, len(manifest.Target))
	for platform := range manifest.Target {
		platforms = append(platforms, platform)
	}
	sort.Strings(platforms)
	for _, platform := range platforms {
		target := manifest.Target[platform]
		reason := fmt.Sprintf("target-specific (%s)", platform)
		for _, table := range []map[string]any{target.Dependencies, target.DevDependencies, target.BuildDependencies} {
			if err := unsynced(table, reason); err != nil {
				return mapping, err
			}
		}
	}

	sortDeps(mapping.Deps)
	sortDeps(mapping.TestDeps)
	return mapping, nil
}

// mapCargoDep maps a resolved dependency to its Buck2 target: a workspace
// member's target, or the external cell's target for the package. It
// reports false if the dependency is neither.
func (m *Mapper) mapCargoDep(dep cargoDep) (MappedDep, bool) {
	cfg := m.config.Rust

	memberDir, isMember := cfg.WorkspacePackages[dep.Package]
	if dep.Path != "" {
		rel, err := filepath.Rel(m.config.ProjectRoot, dep.Path)
		if err != nil || rel == ".." || strings.HasPrefix(rel, "../") {
			return MappedDep{}, false
		}
		memberDir, isMember = filepath.ToSlash(rel), true
	}
	if isMember {
		// e.g. "src/rust/nix-eval" -> "//src/rust/nix-eval:nix-eval"
		return MappedDep{
			Target:     fmt.Sprintf("//%s:%s", memberDir, filepath.Base(memberDir)),
			Type:       DependencyInternal,
			ImportPath: dep.Package,
		}, true
	}

	if !m.isKnownRustDep(dep.Package) {
		return MappedDep{}, false
	}
	// e.g. "tree-sitter" -> "rustdeps//vendor/tree-sitter:tree-sitter"
	return MappedDep{
		Target:     fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, dep.Package, dep.Package),
		Type:       DependencyExternal,
		ImportPath: dep.Package,
	}, true
}

// sortDeps sorts deps by target, for stable output.
func sortDeps(deps []MappedDep) {
	sort.Slice(deps, func(i, j int) bool {
		return deps[i].Target < deps[j].Target
	})
}
