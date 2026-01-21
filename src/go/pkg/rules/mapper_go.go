package rules

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// GoDepsFile represents the structure of go-deps.toml.
type GoDepsFile struct {
	SchemaVersion int                   `toml:"schema_version"`
	Deps          map[string]GoDepsEntry `toml:"deps"`
}

// GoDepsEntry represents a single dependency in go-deps.toml.
type GoDepsEntry struct {
	Version  string `toml:"version"`
	Hash     string `toml:"hash"`
	Indirect bool   `toml:"indirect,omitempty"`
}

// GoMapper maps Go imports to Buck2 target paths.
type GoMapper struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// ModulePath is the Go module path (from go.mod).
	ModulePath string

	// InternalPrefix is the Buck2 prefix for internal targets (e.g., "//src/go").
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps (e.g., "godeps").
	ExternalCell string

	// ExternalDeps maps import paths to their go-deps.toml entries.
	ExternalDeps map[string]GoDepsEntry
}

// NewGoMapper creates a new Go import mapper.
func NewGoMapper(projectRoot string) (*GoMapper, error) {
	m := &GoMapper{
		ProjectRoot:    projectRoot,
		InternalPrefix: "//src/go",      // Default, can be configured
		ExternalCell:   "godeps",         // Default, can be configured
		ExternalDeps:   make(map[string]GoDepsEntry),
	}

	// Read module path from go.mod
	modPath := filepath.Join(projectRoot, "go.mod")
	if content, err := os.ReadFile(modPath); err == nil {
		m.ModulePath = extractModulePath(string(content))
	}

	// Load go-deps.toml
	depsPath := filepath.Join(projectRoot, "go-deps.toml")
	if err := m.loadDepsFile(depsPath); err != nil {
		// Not fatal - deps file might not exist yet
	}

	return m, nil
}

// loadDepsFile loads and parses go-deps.toml.
func (m *GoMapper) loadDepsFile(path string) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var depsFile GoDepsFile
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return err
	}

	m.ExternalDeps = depsFile.Deps
	return nil
}

// MapImport maps a single Go import to a Buck2 dependency.
// Returns nil if the import should be ignored (stdlib, etc.).
func (m *GoMapper) MapImport(imp Import) *Dependency {
	// Skip standard library
	if imp.IsStdLib {
		return &Dependency{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}
	}

	// Check if it's an internal import
	if m.isInternal(imp.Path) {
		return m.mapInternalImport(imp.Path)
	}

	// Check if it's in go-deps.toml (external)
	if m.isExternal(imp.Path) {
		return m.mapExternalImport(imp.Path)
	}

	// Unknown import - might be a missing dependency
	return nil
}

// isInternal checks if an import is internal to the monorepo.
func (m *GoMapper) isInternal(importPath string) bool {
	if m.ModulePath == "" {
		return false
	}
	return strings.HasPrefix(importPath, m.ModulePath)
}

// isExternal checks if an import is in go-deps.toml.
func (m *GoMapper) isExternal(importPath string) bool {
	// Check exact match first
	if _, ok := m.ExternalDeps[importPath]; ok {
		return true
	}

	// Check if any registered dep is a prefix
	// e.g., "github.com/google/uuid" covers "github.com/google/uuid/sub"
	for dep := range m.ExternalDeps {
		if strings.HasPrefix(importPath, dep+"/") || importPath == dep {
			return true
		}
	}

	return false
}

// mapInternalImport maps an internal import to a Buck2 target.
func (m *GoMapper) mapInternalImport(importPath string) *Dependency {
	// Remove module path prefix
	relPath := strings.TrimPrefix(importPath, m.ModulePath+"/")

	// Convert to Buck2 target path
	// e.g., "src/go/pkg/foo" -> "//src/go/pkg/foo:foo"
	targetName := filepath.Base(relPath)
	buckPath := fmt.Sprintf("//%s:%s", relPath, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyInternal,
		ImportPath: importPath,
	}
}

// mapExternalImport maps an external import to a Buck2 target.
func (m *GoMapper) mapExternalImport(importPath string) *Dependency {
	// Use the full import path for the target
	// e.g., "golang.org/x/sys/cpu" -> "godeps//vendor/golang.org/x/sys/cpu:cpu"
	// e.g., "github.com/google/uuid" -> "godeps//vendor/github.com/google/uuid:uuid"
	targetName := filepath.Base(importPath)
	buckPath := fmt.Sprintf("%s//vendor/%s:%s", m.ExternalCell, importPath, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// findRootDep finds the root dependency for a sub-package import.
// e.g., "golang.org/x/sys/cpu" -> "golang.org/x/sys"
func (m *GoMapper) findRootDep(importPath string) string {
	// Check each registered dep to see if it's a prefix
	for dep := range m.ExternalDeps {
		if strings.HasPrefix(importPath, dep+"/") || importPath == dep {
			return dep
		}
	}
	return ""
}

// MapImports maps multiple imports to Buck2 dependencies.
// Returns both the dependencies and any unmapped imports.
func (m *GoMapper) MapImports(imports []Import) (deps []Dependency, unmapped []Import) {
	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.MapImport(imp)

		if dep == nil {
			unmapped = append(unmapped, imp)
			continue
		}

		if dep.Type == DependencyStdLib {
			// Skip stdlib
			continue
		}

		// Deduplicate
		if !seen[dep.Target] {
			seen[dep.Target] = true
			deps = append(deps, *dep)
		}
	}

	return deps, unmapped
}

// DepsToTargets extracts just the target strings from dependencies.
func DepsToTargets(deps []Dependency) []string {
	var targets []string
	for _, dep := range deps {
		targets = append(targets, dep.Target)
	}
	return targets
}

// SetInternalPrefix configures the internal prefix.
func (m *GoMapper) SetInternalPrefix(prefix string) {
	m.InternalPrefix = prefix
}

// SetExternalCell configures the external cell name.
func (m *GoMapper) SetExternalCell(cell string) {
	m.ExternalCell = cell
}
