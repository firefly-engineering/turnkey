package rules

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/pelletier/go-toml/v2"
)

// PythonDepsFile represents the structure of python-deps.toml.
type PythonDepsFile struct {
	SchemaVersion int                       `toml:"schema_version"`
	Deps          map[string]PythonDepsEntry `toml:"deps"`
}

// PythonDepsEntry represents a single dependency in python-deps.toml.
type PythonDepsEntry struct {
	Version string `toml:"version"`
	Hash    string `toml:"hash"`
	URL     string `toml:"url"`
}

// PythonMapper maps Python module imports to Buck2 target paths.
type PythonMapper struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// InternalPrefix is the Buck2 prefix for internal targets (e.g., "//src/python").
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps (e.g., "pydeps").
	ExternalCell string

	// ExternalDeps maps module names to their python-deps.toml entries.
	ExternalDeps map[string]PythonDepsEntry
}

// NewPythonMapper creates a new Python module mapper.
func NewPythonMapper(projectRoot string) (*PythonMapper, error) {
	m := &PythonMapper{
		ProjectRoot:    projectRoot,
		InternalPrefix: "//src/python", // Default, can be configured
		ExternalCell:   "pydeps",       // Default, can be configured
		ExternalDeps:   make(map[string]PythonDepsEntry),
	}

	// Load python-deps.toml
	depsPath := filepath.Join(projectRoot, "python-deps.toml")
	if err := m.loadDepsFile(depsPath); err != nil {
		// Not fatal - deps file might not exist yet
	}

	return m, nil
}

// loadDepsFile loads and parses python-deps.toml.
func (m *PythonMapper) loadDepsFile(path string) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var depsFile PythonDepsFile
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return err
	}

	m.ExternalDeps = depsFile.Deps
	return nil
}

// MapImport maps a single Python module import to a Buck2 dependency.
// Returns nil if the import should be ignored (stdlib, etc.).
func (m *PythonMapper) MapImport(imp Import) *Dependency {
	// Skip standard library
	if imp.IsStdLib {
		return &Dependency{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}
	}

	// Check if it's in python-deps.toml (external)
	if m.isExternal(imp.Path) {
		return m.mapExternalImport(imp.Path)
	}

	// Check if it's internal
	if m.isInternal(imp.Path) {
		return m.mapInternalImport(imp.Path)
	}

	// Unknown import - might be a missing dependency
	return nil
}

// isInternal checks if a module is internal to the monorepo.
func (m *PythonMapper) isInternal(moduleName string) bool {
	// Check if it's a local package
	packageDir := filepath.Join(m.ProjectRoot, "src", "python", moduleName)
	if _, err := os.Stat(filepath.Join(packageDir, "__init__.py")); err == nil {
		return true
	}
	return false
}

// isExternal checks if a module is in python-deps.toml.
func (m *PythonMapper) isExternal(moduleName string) bool {
	_, ok := m.ExternalDeps[moduleName]
	return ok
}

// mapInternalImport maps an internal module to a Buck2 target.
func (m *PythonMapper) mapInternalImport(moduleName string) *Dependency {
	// Convert to Buck2 target path
	buckPath := fmt.Sprintf("%s/%s:%s", m.InternalPrefix, moduleName, moduleName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyInternal,
		ImportPath: moduleName,
	}
}

// mapExternalImport maps an external module to a Buck2 target.
func (m *PythonMapper) mapExternalImport(moduleName string) *Dependency {
	// Buck2 target format: pydeps//vendor/{module_name}:{module_name}
	buckPath := fmt.Sprintf("%s//vendor/%s:%s", m.ExternalCell, moduleName, moduleName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyExternal,
		ImportPath: moduleName,
	}
}

// MapImports maps multiple imports to Buck2 dependencies.
func (m *PythonMapper) MapImports(imports []Import) (deps []Dependency, unmapped []Import) {
	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.MapImport(imp)

		if dep == nil {
			unmapped = append(unmapped, imp)
			continue
		}

		if dep.Type == DependencyStdLib {
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

// SetInternalPrefix configures the internal prefix.
func (m *PythonMapper) SetInternalPrefix(prefix string) {
	m.InternalPrefix = prefix
}

// SetExternalCell configures the external cell name.
func (m *PythonMapper) SetExternalCell(cell string) {
	m.ExternalCell = cell
}
