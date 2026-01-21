package rules

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// JSDepsFile represents the structure of js-deps.toml.
type JSDepsFile struct {
	Meta     JSDepsFileMeta  `toml:"meta"`
	Packages []JSDepsPackage `toml:"package"`
}

// JSDepsFileMeta contains metadata about the deps file.
type JSDepsFileMeta struct {
	Generator       string `toml:"generator"`
	LockfileVersion string `toml:"lockfile_version"`
}

// JSDepsPackage represents a single package in js-deps.toml.
type JSDepsPackage struct {
	Name      string `toml:"name"`
	Version   string `toml:"version"`
	URL       string `toml:"url"`
	Integrity string `toml:"integrity"`
}

// TypeScriptMapper maps TypeScript/JavaScript imports to Buck2 target paths.
type TypeScriptMapper struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// InternalPrefix is the Buck2 prefix for internal targets (e.g., "//src/ts").
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps (e.g., "jsdeps").
	ExternalCell string

	// ExternalDeps maps package names to their js-deps.toml entries.
	ExternalDeps map[string]JSDepsPackage
}

// NewTypeScriptMapper creates a new TypeScript/JavaScript module mapper.
func NewTypeScriptMapper(projectRoot string) (*TypeScriptMapper, error) {
	m := &TypeScriptMapper{
		ProjectRoot:    projectRoot,
		InternalPrefix: "//src/ts",  // Default, can be configured
		ExternalCell:   "jsdeps",    // Default, can be configured
		ExternalDeps:   make(map[string]JSDepsPackage),
	}

	// Load js-deps.toml
	depsPath := filepath.Join(projectRoot, "js-deps.toml")
	if err := m.loadDepsFile(depsPath); err != nil {
		// Not fatal - deps file might not exist yet
	}

	return m, nil
}

// loadDepsFile loads and parses js-deps.toml.
func (m *TypeScriptMapper) loadDepsFile(path string) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var depsFile JSDepsFile
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return err
	}

	// Index by package name
	for _, pkg := range depsFile.Packages {
		m.ExternalDeps[pkg.Name] = pkg
	}

	return nil
}

// MapImport maps a single TypeScript/JavaScript import to a Buck2 dependency.
// Returns nil if the import should be ignored (built-in, etc.).
func (m *TypeScriptMapper) MapImport(imp Import) *Dependency {
	// Skip Node.js built-ins
	if imp.IsStdLib {
		return &Dependency{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}
	}

	// Check if it's in js-deps.toml (external)
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
func (m *TypeScriptMapper) isInternal(moduleName string) bool {
	// Check if it's a local package
	packageDir := filepath.Join(m.ProjectRoot, "src", "ts", moduleName)
	if _, err := os.Stat(filepath.Join(packageDir, "package.json")); err == nil {
		return true
	}
	return false
}

// isExternal checks if a module is in js-deps.toml.
func (m *TypeScriptMapper) isExternal(moduleName string) bool {
	_, ok := m.ExternalDeps[moduleName]
	return ok
}

// mapInternalImport maps an internal module to a Buck2 target.
func (m *TypeScriptMapper) mapInternalImport(moduleName string) *Dependency {
	// Convert to Buck2 target path
	buckPath := fmt.Sprintf("%s/%s:%s", m.InternalPrefix, moduleName, moduleName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyInternal,
		ImportPath: moduleName,
	}
}

// mapExternalImport maps an external module to a Buck2 target.
func (m *TypeScriptMapper) mapExternalImport(moduleName string) *Dependency {
	// Convert package name to Buck2 target name
	// @types/lodash -> types_lodash
	// @org/pkg -> org_pkg
	// lodash -> lodash
	targetName := packageNameToTargetName(moduleName)

	// Buck2 target format for JS: jsdeps//:target_name
	buckPath := fmt.Sprintf("%s//:%s", m.ExternalCell, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyExternal,
		ImportPath: moduleName,
	}
}

// packageNameToTargetName converts an npm package name to a Buck2 target name.
// Handles scoped packages: @types/lodash -> types_lodash
func packageNameToTargetName(packageName string) string {
	// Remove @ prefix for scoped packages
	name := strings.TrimPrefix(packageName, "@")
	// Replace / and - with _
	name = strings.ReplaceAll(name, "/", "_")
	name = strings.ReplaceAll(name, "-", "_")
	return name
}

// MapImports maps multiple imports to Buck2 dependencies.
func (m *TypeScriptMapper) MapImports(imports []Import) (deps []Dependency, unmapped []Import) {
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
func (m *TypeScriptMapper) SetInternalPrefix(prefix string) {
	m.InternalPrefix = prefix
}

// SetExternalCell configures the external cell name.
func (m *TypeScriptMapper) SetExternalCell(cell string) {
	m.ExternalCell = cell
}
