// Package mapper converts extraction results to Buck2 target references
// and applies them to rules.star files using the starlark object model.
package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/pelletier/go-toml/v2"
)

// DependencyType classifies a dependency.
type DependencyType int

const (
	DependencyStdLib DependencyType = iota
	DependencyInternal
	DependencyExternal
	DependencyUnmapped
)

// MappedDep represents a resolved Buck2 dependency.
type MappedDep struct {
	// Target is the Buck2 target path (e.g., "//src/go/pkg/foo:foo").
	Target string

	// Type classifies the dependency.
	Type DependencyType

	// ImportPath is the original import path.
	ImportPath string
}

// Config holds mapper configuration.
type Config struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// Language-specific configuration
	Go *GoConfig
}

// GoConfig holds Go-specific configuration.
type GoConfig struct {
	// ModulePath is the Go module path (from go.mod).
	ModulePath string

	// ExternalCell is the Buck2 cell for external deps (e.g., "godeps").
	ExternalCell string

	// DepsFile is the path to go-deps.toml.
	DepsFile string

	// ExternalDeps maps import paths to their entries from go-deps.toml.
	ExternalDeps map[string]bool
}

// Mapper converts extraction results to Buck2 targets.
type Mapper struct {
	config Config
}

// New creates a new Mapper with the given configuration.
func New(cfg Config) (*Mapper, error) {
	m := &Mapper{config: cfg}

	// Auto-detect Go configuration if not provided
	if cfg.Go == nil {
		goCfg, err := detectGoConfig(cfg.ProjectRoot)
		if err == nil {
			m.config.Go = goCfg
		}
	} else {
		// Load external deps if not already loaded
		if cfg.Go.ExternalDeps == nil && cfg.Go.DepsFile != "" {
			if deps, err := loadGoDeps(cfg.Go.DepsFile); err == nil {
				m.config.Go.ExternalDeps = deps
			}
		}
	}

	return m, nil
}

// detectGoConfig auto-detects Go configuration from the project.
func detectGoConfig(projectRoot string) (*GoConfig, error) {
	cfg := &GoConfig{
		ExternalCell: "godeps",
		ExternalDeps: make(map[string]bool),
	}

	// Read module path from go.mod
	modPath := filepath.Join(projectRoot, "go.mod")
	if content, err := os.ReadFile(modPath); err == nil {
		cfg.ModulePath = extractModulePath(string(content))
	}

	// Load go-deps.toml
	depsPath := filepath.Join(projectRoot, "go-deps.toml")
	if deps, err := loadGoDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// extractModulePath extracts the module path from go.mod content.
func extractModulePath(content string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "module ") {
			return strings.TrimSpace(strings.TrimPrefix(line, "module "))
		}
	}
	return ""
}

// loadGoDeps loads dependency names from go-deps.toml.
func loadGoDeps(path string) (map[string]bool, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var depsFile struct {
		Deps map[string]interface{} `toml:"deps"`
	}
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return nil, err
	}

	result := make(map[string]bool)
	for dep := range depsFile.Deps {
		result[dep] = true
	}
	return result, nil
}

// MapExtractionResult converts an extraction result to mapped dependencies.
func (m *Mapper) MapExtractionResult(result *extraction.Result) (map[string]PackageMapping, error) {
	mappings := make(map[string]PackageMapping)

	for _, pkg := range result.Packages {
		mapping := m.mapPackage(result.Language, pkg)
		mappings[pkg.Path] = mapping
	}

	return mappings, nil
}

// PackageMapping contains the mapped dependencies for a package.
type PackageMapping struct {
	// Path is the package path.
	Path string

	// Deps are the resolved dependencies for the library target.
	Deps []MappedDep

	// TestDeps are the resolved dependencies for the test target.
	TestDeps []MappedDep

	// UnmappedImports are imports that couldn't be mapped.
	UnmappedImports []string
}

// mapPackage maps a single package's imports to dependencies.
func (m *Mapper) mapPackage(language string, pkg extraction.Package) PackageMapping {
	mapping := PackageMapping{Path: pkg.Path}

	switch language {
	case "go":
		mapping.Deps, mapping.UnmappedImports = m.mapGoImports(pkg.Imports)
		testDeps, testUnmapped := m.mapGoImports(pkg.TestImports)
		mapping.TestDeps = testDeps
		mapping.UnmappedImports = append(mapping.UnmappedImports, testUnmapped...)
	default:
		// Unknown language, report all as unmapped
		for _, imp := range pkg.Imports {
			mapping.UnmappedImports = append(mapping.UnmappedImports, imp.Path)
		}
	}

	return mapping
}

// mapGoImports maps Go imports to Buck2 dependencies.
func (m *Mapper) mapGoImports(imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	if m.config.Go == nil {
		for _, imp := range imports {
			unmapped = append(unmapped, imp.Path)
		}
		return
	}

	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.mapGoImport(imp)

		switch dep.Type {
		case DependencyStdLib:
			// Skip stdlib
			continue
		case DependencyUnmapped:
			unmapped = append(unmapped, imp.Path)
			continue
		}

		// Deduplicate
		if !seen[dep.Target] {
			seen[dep.Target] = true
			deps = append(deps, dep)
		}
	}

	// Sort for consistent output
	sort.Slice(deps, func(i, j int) bool {
		return deps[i].Target < deps[j].Target
	})

	return deps, unmapped
}

// mapGoImport maps a single Go import to a Buck2 dependency.
func (m *Mapper) mapGoImport(imp extraction.Import) MappedDep {
	// Handle by kind
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return MappedDep{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}

	case extraction.ImportKindInternal:
		return m.mapGoInternalImport(imp.Path)

	case extraction.ImportKindExternal:
		return m.mapGoExternalImport(imp.Path)
	}

	return MappedDep{
		ImportPath: imp.Path,
		Type:       DependencyUnmapped,
	}
}

// mapGoInternalImport maps an internal Go import to a Buck2 target.
func (m *Mapper) mapGoInternalImport(importPath string) MappedDep {
	cfg := m.config.Go

	// Remove module path prefix to get relative path
	relPath := strings.TrimPrefix(importPath, cfg.ModulePath+"/")

	// Convert to Buck2 target path
	// e.g., "src/go/pkg/foo" -> "//src/go/pkg/foo:foo"
	targetName := filepath.Base(relPath)
	target := fmt.Sprintf("//%s:%s", relPath, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyInternal,
		ImportPath: importPath,
	}
}

// mapGoExternalImport maps an external Go import to a Buck2 target.
func (m *Mapper) mapGoExternalImport(importPath string) MappedDep {
	cfg := m.config.Go

	// Check if this import or a parent is in go-deps.toml
	if !m.isKnownExternalDep(importPath) {
		return MappedDep{
			ImportPath: importPath,
			Type:       DependencyUnmapped,
		}
	}

	// Use the full import path for the target
	// e.g., "golang.org/x/sys/cpu" -> "godeps//vendor/golang.org/x/sys/cpu:cpu"
	targetName := filepath.Base(importPath)
	target := fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, importPath, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// isKnownExternalDep checks if an import is in go-deps.toml or is a subpackage.
func (m *Mapper) isKnownExternalDep(importPath string) bool {
	if m.config.Go == nil || m.config.Go.ExternalDeps == nil {
		return false
	}

	// Check exact match
	if m.config.Go.ExternalDeps[importPath] {
		return true
	}

	// Check if any registered dep is a prefix
	for dep := range m.config.Go.ExternalDeps {
		if strings.HasPrefix(importPath, dep+"/") {
			return true
		}
	}

	return false
}

// ApplyToRulesStar applies mapped dependencies to a rules.star file.
func (m *Mapper) ApplyToRulesStar(rulesPath string, pkgMapping PackageMapping) error {
	// Parse the rules.star file
	f, err := starlark.ParseFile(rulesPath)
	if err != nil {
		return fmt.Errorf("parsing rules.star: %w", err)
	}

	// Find the library target (typically matches the directory name)
	dirName := filepath.Base(filepath.Dir(rulesPath))

	// Try common library target names
	var libTarget *starlark.Target
	for _, name := range []string{dirName, "lib", "library"} {
		libTarget = f.GetTarget(name)
		if libTarget != nil {
			break
		}
	}

	if libTarget != nil && len(pkgMapping.Deps) > 0 {
		// Convert MappedDep to string slice
		var deps []string
		for _, d := range pkgMapping.Deps {
			deps = append(deps, d.Target)
		}
		libTarget.SetDeps(deps)
	}

	// Find the test target
	var testTarget *starlark.Target
	for _, name := range []string{dirName + "_test", "test", "tests"} {
		testTarget = f.GetTarget(name)
		if testTarget != nil {
			break
		}
	}

	if testTarget != nil && len(pkgMapping.TestDeps) > 0 {
		// For tests, we need to include both regular deps and test-only deps
		var testDeps []string
		for _, d := range pkgMapping.Deps {
			testDeps = append(testDeps, d.Target)
		}
		for _, d := range pkgMapping.TestDeps {
			testDeps = append(testDeps, d.Target)
		}
		testTarget.SetDeps(testDeps)
	}

	// Write back if modified
	if f.IsModified() {
		output := f.Write()
		if err := os.WriteFile(rulesPath, output, 0644); err != nil {
			return fmt.Errorf("writing rules.star: %w", err)
		}
	}

	return nil
}

// DepsToTargets extracts just the target strings from mapped deps.
func DepsToTargets(deps []MappedDep) []string {
	var targets []string
	for _, dep := range deps {
		targets = append(targets, dep.Target)
	}
	return targets
}
