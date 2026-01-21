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
	Go         *GoConfig
	Rust       *RustConfig
	Python     *PythonConfig
	TypeScript *TypeScriptConfig
	Solidity   *SolidityConfig
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

// RustConfig holds Rust-specific configuration.
type RustConfig struct {
	// WorkspaceRoot is the Cargo workspace root directory.
	WorkspaceRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "rustdeps").
	ExternalCell string

	// DepsFile is the path to rust-deps.toml.
	DepsFile string

	// ExternalDeps maps crate names to their entries from rust-deps.toml.
	ExternalDeps map[string]bool

	// WorkspacePackages maps crate names to their relative paths.
	WorkspacePackages map[string]string
}

// PythonConfig holds Python-specific configuration.
type PythonConfig struct {
	// ProjectRoot is the Python project root directory.
	ProjectRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "pydeps").
	ExternalCell string

	// DepsFile is the path to python-deps.toml.
	DepsFile string

	// ExternalDeps maps package names to their entries from python-deps.toml.
	ExternalDeps map[string]bool
}

// TypeScriptConfig holds TypeScript/JavaScript-specific configuration.
type TypeScriptConfig struct {
	// ProjectRoot is the TypeScript project root directory.
	ProjectRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "jsdeps").
	ExternalCell string

	// DepsFile is the path to js-deps.toml.
	DepsFile string

	// ExternalDeps maps package names to their entries from js-deps.toml.
	ExternalDeps map[string]bool
}

// SolidityConfig holds Solidity-specific configuration.
type SolidityConfig struct {
	// ProjectRoot is the Solidity project root directory.
	ProjectRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "soldeps").
	ExternalCell string

	// DepsFile is the path to sol-deps.toml.
	DepsFile string

	// ExternalDeps maps package names to their entries from sol-deps.toml.
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

	// Auto-detect Rust configuration if not provided
	if cfg.Rust == nil {
		rustCfg, err := detectRustConfig(cfg.ProjectRoot)
		if err == nil {
			m.config.Rust = rustCfg
		}
	} else {
		// Load external deps if not already loaded
		if cfg.Rust.ExternalDeps == nil && cfg.Rust.DepsFile != "" {
			if deps, err := loadRustDeps(cfg.Rust.DepsFile); err == nil {
				m.config.Rust.ExternalDeps = deps
			}
		}
	}

	// Auto-detect Python configuration if not provided
	if cfg.Python == nil {
		pythonCfg, err := detectPythonConfig(cfg.ProjectRoot)
		if err == nil {
			m.config.Python = pythonCfg
		}
	} else {
		// Load external deps if not already loaded
		if cfg.Python.ExternalDeps == nil && cfg.Python.DepsFile != "" {
			if deps, err := loadPythonDeps(cfg.Python.DepsFile); err == nil {
				m.config.Python.ExternalDeps = deps
			}
		}
	}

	// Auto-detect TypeScript configuration if not provided
	if cfg.TypeScript == nil {
		tsCfg, err := detectTypescriptConfig(cfg.ProjectRoot)
		if err == nil {
			m.config.TypeScript = tsCfg
		}
	} else {
		// Load external deps if not already loaded
		if cfg.TypeScript.ExternalDeps == nil && cfg.TypeScript.DepsFile != "" {
			if deps, err := loadTypescriptDeps(cfg.TypeScript.DepsFile); err == nil {
				m.config.TypeScript.ExternalDeps = deps
			}
		}
	}

	// Auto-detect Solidity configuration if not provided
	if cfg.Solidity == nil {
		solCfg, err := detectSolidityConfig(cfg.ProjectRoot)
		if err == nil {
			m.config.Solidity = solCfg
		}
	} else {
		// Load external deps if not already loaded
		if cfg.Solidity.ExternalDeps == nil && cfg.Solidity.DepsFile != "" {
			if deps, err := loadSolidityDeps(cfg.Solidity.DepsFile); err == nil {
				m.config.Solidity.ExternalDeps = deps
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

// detectRustConfig auto-detects Rust configuration from the project.
func detectRustConfig(projectRoot string) (*RustConfig, error) {
	cfg := &RustConfig{
		ExternalCell:      "rustdeps",
		ExternalDeps:      make(map[string]bool),
		WorkspacePackages: make(map[string]string),
	}

	// Check for Cargo.toml
	cargoPath := filepath.Join(projectRoot, "Cargo.toml")
	if _, err := os.Stat(cargoPath); err != nil {
		return nil, fmt.Errorf("no Cargo.toml found")
	}
	cfg.WorkspaceRoot = projectRoot

	// Load rust-deps.toml
	depsPath := filepath.Join(projectRoot, "rust-deps.toml")
	if deps, err := loadRustDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadRustDeps loads crate names from rust-deps.toml.
func loadRustDeps(path string) (map[string]bool, error) {
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
		// rust-deps.toml uses "crate@version" as keys, extract just the crate name
		if idx := strings.Index(dep, "@"); idx > 0 {
			result[dep[:idx]] = true
		} else {
			result[dep] = true
		}
	}
	return result, nil
}

// detectPythonConfig auto-detects Python configuration from the project.
func detectPythonConfig(projectRoot string) (*PythonConfig, error) {
	cfg := &PythonConfig{
		ExternalCell: "pydeps",
		ExternalDeps: make(map[string]bool),
	}

	// Check for pyproject.toml
	pyprojectPath := filepath.Join(projectRoot, "pyproject.toml")
	if _, err := os.Stat(pyprojectPath); err != nil {
		return nil, fmt.Errorf("no pyproject.toml found")
	}
	cfg.ProjectRoot = projectRoot

	// Load python-deps.toml
	depsPath := filepath.Join(projectRoot, "python-deps.toml")
	if deps, err := loadPythonDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadPythonDeps loads package names from python-deps.toml.
func loadPythonDeps(path string) (map[string]bool, error) {
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

// detectTypescriptConfig auto-detects TypeScript configuration from the project.
func detectTypescriptConfig(projectRoot string) (*TypeScriptConfig, error) {
	cfg := &TypeScriptConfig{
		ExternalCell: "jsdeps",
		ExternalDeps: make(map[string]bool),
	}

	// Check for package.json or tsconfig.json
	packagePath := filepath.Join(projectRoot, "package.json")
	tsconfigPath := filepath.Join(projectRoot, "tsconfig.json")
	if _, err := os.Stat(packagePath); err != nil {
		if _, err := os.Stat(tsconfigPath); err != nil {
			return nil, fmt.Errorf("no package.json or tsconfig.json found")
		}
	}
	cfg.ProjectRoot = projectRoot

	// Load js-deps.toml
	depsPath := filepath.Join(projectRoot, "js-deps.toml")
	if deps, err := loadTypescriptDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadTypescriptDeps loads package names from js-deps.toml.
func loadTypescriptDeps(path string) (map[string]bool, error) {
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

// detectSolidityConfig auto-detects Solidity configuration from the project.
func detectSolidityConfig(projectRoot string) (*SolidityConfig, error) {
	cfg := &SolidityConfig{
		ExternalCell: "soldeps",
		ExternalDeps: make(map[string]bool),
	}

	// Check for foundry.toml or hardhat.config.js/ts
	foundryPath := filepath.Join(projectRoot, "foundry.toml")
	hardhatJsPath := filepath.Join(projectRoot, "hardhat.config.js")
	hardhatTsPath := filepath.Join(projectRoot, "hardhat.config.ts")
	if _, err := os.Stat(foundryPath); err != nil {
		if _, err := os.Stat(hardhatJsPath); err != nil {
			if _, err := os.Stat(hardhatTsPath); err != nil {
				return nil, fmt.Errorf("no foundry.toml or hardhat.config found")
			}
		}
	}
	cfg.ProjectRoot = projectRoot

	// Load sol-deps.toml
	depsPath := filepath.Join(projectRoot, "sol-deps.toml")
	if deps, err := loadSolidityDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadSolidityDeps loads package names from sol-deps.toml.
func loadSolidityDeps(path string) (map[string]bool, error) {
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
	case "rust":
		mapping.Deps, mapping.UnmappedImports = m.mapRustImports(pkg.Imports)
		testDeps, testUnmapped := m.mapRustImports(pkg.TestImports)
		mapping.TestDeps = testDeps
		mapping.UnmappedImports = append(mapping.UnmappedImports, testUnmapped...)
	case "python":
		mapping.Deps, mapping.UnmappedImports = m.mapPythonImports(pkg.Imports)
		testDeps, testUnmapped := m.mapPythonImports(pkg.TestImports)
		mapping.TestDeps = testDeps
		mapping.UnmappedImports = append(mapping.UnmappedImports, testUnmapped...)
	case "typescript":
		mapping.Deps, mapping.UnmappedImports = m.mapTypescriptImports(pkg.Imports)
		testDeps, testUnmapped := m.mapTypescriptImports(pkg.TestImports)
		mapping.TestDeps = testDeps
		mapping.UnmappedImports = append(mapping.UnmappedImports, testUnmapped...)
	case "solidity":
		mapping.Deps, mapping.UnmappedImports = m.mapSolidityImports(pkg.Imports)
		testDeps, testUnmapped := m.mapSolidityImports(pkg.TestImports)
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

// mapRustImports maps Rust imports to Buck2 dependencies.
func (m *Mapper) mapRustImports(imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	if m.config.Rust == nil {
		for _, imp := range imports {
			unmapped = append(unmapped, imp.Path)
		}
		return
	}

	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.mapRustImport(imp)

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

// mapRustImport maps a single Rust import to a Buck2 dependency.
func (m *Mapper) mapRustImport(imp extraction.Import) MappedDep {
	// Handle by kind
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return MappedDep{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}

	case extraction.ImportKindInternal:
		return m.mapRustInternalImport(imp.Path)

	case extraction.ImportKindExternal:
		return m.mapRustExternalImport(imp.Path)
	}

	return MappedDep{
		ImportPath: imp.Path,
		Type:       DependencyUnmapped,
	}
}

// mapRustInternalImport maps an internal Rust crate to a Buck2 target.
func (m *Mapper) mapRustInternalImport(crateName string) MappedDep {
	cfg := m.config.Rust

	// Check if this crate is in the workspace
	if relPath, ok := cfg.WorkspacePackages[crateName]; ok {
		// Convert to Buck2 target path
		// e.g., "src/rust/prefetch-cache" -> "//src/rust/prefetch-cache:prefetch-cache"
		targetName := filepath.Base(relPath)
		target := fmt.Sprintf("//%s:%s", relPath, targetName)

		return MappedDep{
			Target:     target,
			Type:       DependencyInternal,
			ImportPath: crateName,
		}
	}

	// Unknown internal dep - treat as unmapped for now
	return MappedDep{
		ImportPath: crateName,
		Type:       DependencyUnmapped,
	}
}

// mapRustExternalImport maps an external Rust crate to a Buck2 target.
func (m *Mapper) mapRustExternalImport(crateName string) MappedDep {
	cfg := m.config.Rust

	// Check if this crate is in rust-deps.toml
	if !m.isKnownRustDep(crateName) {
		return MappedDep{
			ImportPath: crateName,
			Type:       DependencyUnmapped,
		}
	}

	// Use the crate name for the target
	// e.g., "serde" -> "rustdeps//vendor/serde:serde"
	target := fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, crateName, crateName)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: crateName,
	}
}

// isKnownRustDep checks if a crate is in rust-deps.toml.
func (m *Mapper) isKnownRustDep(crateName string) bool {
	if m.config.Rust == nil || m.config.Rust.ExternalDeps == nil {
		return false
	}

	return m.config.Rust.ExternalDeps[crateName]
}

// mapPythonImports maps Python imports to Buck2 dependencies.
func (m *Mapper) mapPythonImports(imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	if m.config.Python == nil {
		for _, imp := range imports {
			unmapped = append(unmapped, imp.Path)
		}
		return
	}

	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.mapPythonImport(imp)

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

// mapPythonImport maps a single Python import to a Buck2 dependency.
func (m *Mapper) mapPythonImport(imp extraction.Import) MappedDep {
	// Handle by kind
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return MappedDep{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}

	case extraction.ImportKindInternal:
		return m.mapPythonInternalImport(imp.Path)

	case extraction.ImportKindExternal:
		return m.mapPythonExternalImport(imp.Path)
	}

	return MappedDep{
		ImportPath: imp.Path,
		Type:       DependencyUnmapped,
	}
}

// mapPythonInternalImport maps an internal Python import to a Buck2 target.
func (m *Mapper) mapPythonInternalImport(modulePath string) MappedDep {
	// Convert Python module path to Buck2 target
	// e.g., "src.python.cfg" -> "//src/python/cfg:cfg"
	parts := strings.Split(modulePath, ".")
	relPath := strings.Join(parts, "/")
	targetName := parts[len(parts)-1]

	target := fmt.Sprintf("//%s:%s", relPath, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyInternal,
		ImportPath: modulePath,
	}
}

// mapPythonExternalImport maps an external Python import to a Buck2 target.
func (m *Mapper) mapPythonExternalImport(modulePath string) MappedDep {
	cfg := m.config.Python

	// Get top-level package name
	topLevel := modulePath
	if idx := strings.Index(modulePath, "."); idx > 0 {
		topLevel = modulePath[:idx]
	}

	// Check if this package is in python-deps.toml
	if !m.isKnownPythonDep(topLevel) {
		return MappedDep{
			ImportPath: modulePath,
			Type:       DependencyUnmapped,
		}
	}

	// Use the top-level package name for the target
	// e.g., "requests" -> "pydeps//vendor/requests:requests"
	target := fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, topLevel, topLevel)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: modulePath,
	}
}

// isKnownPythonDep checks if a package is in python-deps.toml.
func (m *Mapper) isKnownPythonDep(packageName string) bool {
	if m.config.Python == nil || m.config.Python.ExternalDeps == nil {
		return false
	}

	return m.config.Python.ExternalDeps[packageName]
}

// mapTypescriptImports maps TypeScript imports to Buck2 dependencies.
func (m *Mapper) mapTypescriptImports(imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	if m.config.TypeScript == nil {
		for _, imp := range imports {
			unmapped = append(unmapped, imp.Path)
		}
		return
	}

	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.mapTypescriptImport(imp)

		switch dep.Type {
		case DependencyStdLib:
			// Skip stdlib (Node.js builtins)
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

// mapTypescriptImport maps a single TypeScript import to a Buck2 dependency.
func (m *Mapper) mapTypescriptImport(imp extraction.Import) MappedDep {
	// Handle by kind
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return MappedDep{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}

	case extraction.ImportKindInternal:
		return m.mapTypescriptInternalImport(imp.Path)

	case extraction.ImportKindExternal:
		return m.mapTypescriptExternalImport(imp.Path)
	}

	return MappedDep{
		ImportPath: imp.Path,
		Type:       DependencyUnmapped,
	}
}

// mapTypescriptInternalImport maps an internal TypeScript import to a Buck2 target.
func (m *Mapper) mapTypescriptInternalImport(modulePath string) MappedDep {
	// Relative imports like ./foo or ../bar are internal
	// Convert relative path to Buck2 target
	// This is complex because we need the context of where the import is from
	// For now, we'll treat relative imports as internal without full path resolution
	return MappedDep{
		Target:     modulePath,
		Type:       DependencyInternal,
		ImportPath: modulePath,
	}
}

// mapTypescriptExternalImport maps an external TypeScript import to a Buck2 target.
func (m *Mapper) mapTypescriptExternalImport(modulePath string) MappedDep {
	cfg := m.config.TypeScript

	// Get the package name (handle scoped packages like @org/pkg)
	pkgName := modulePath
	if strings.HasPrefix(modulePath, "@") {
		// Scoped package: @org/pkg/subpath -> @org/pkg
		parts := strings.SplitN(modulePath, "/", 3)
		if len(parts) >= 2 {
			pkgName = parts[0] + "/" + parts[1]
		}
	} else {
		// Regular package: pkg/subpath -> pkg
		parts := strings.SplitN(modulePath, "/", 2)
		pkgName = parts[0]
	}

	// Check if this package is in js-deps.toml
	if !m.isKnownTypescriptDep(pkgName) {
		return MappedDep{
			ImportPath: modulePath,
			Type:       DependencyUnmapped,
		}
	}

	// Use the package name for the target
	// e.g., "react" -> "jsdeps//vendor/react:react"
	// e.g., "@types/node" -> "jsdeps//vendor/@types/node:node"
	targetName := filepath.Base(pkgName)
	target := fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, pkgName, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: modulePath,
	}
}

// isKnownTypescriptDep checks if a package is in js-deps.toml.
func (m *Mapper) isKnownTypescriptDep(packageName string) bool {
	if m.config.TypeScript == nil || m.config.TypeScript.ExternalDeps == nil {
		return false
	}

	return m.config.TypeScript.ExternalDeps[packageName]
}

// mapSolidityImports maps Solidity imports to Buck2 dependencies.
func (m *Mapper) mapSolidityImports(imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	if m.config.Solidity == nil {
		for _, imp := range imports {
			unmapped = append(unmapped, imp.Path)
		}
		return
	}

	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.mapSolidityImport(imp)

		switch dep.Type {
		case DependencyStdLib:
			// Solidity has no stdlib, skip
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

// mapSolidityImport maps a single Solidity import to a Buck2 dependency.
func (m *Mapper) mapSolidityImport(imp extraction.Import) MappedDep {
	// Handle by kind
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		// Solidity has no stdlib
		return MappedDep{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}

	case extraction.ImportKindInternal:
		return m.mapSolidityInternalImport(imp.Path)

	case extraction.ImportKindExternal:
		return m.mapSolidityExternalImport(imp.Path)
	}

	return MappedDep{
		ImportPath: imp.Path,
		Type:       DependencyUnmapped,
	}
}

// mapSolidityInternalImport maps an internal Solidity import to a Buck2 target.
func (m *Mapper) mapSolidityInternalImport(importPath string) MappedDep {
	// Relative imports like ./Foo.sol or ../Bar.sol are internal
	return MappedDep{
		Target:     importPath,
		Type:       DependencyInternal,
		ImportPath: importPath,
	}
}

// mapSolidityExternalImport maps an external Solidity import to a Buck2 target.
func (m *Mapper) mapSolidityExternalImport(importPath string) MappedDep {
	cfg := m.config.Solidity

	// Get the package name (handle scoped packages like @openzeppelin/contracts)
	pkgName := importPath
	if strings.HasPrefix(importPath, "@") {
		// Scoped package: @org/pkg/subpath -> @org/pkg
		parts := strings.SplitN(importPath, "/", 3)
		if len(parts) >= 2 {
			pkgName = parts[0] + "/" + parts[1]
		}
	} else {
		// Regular package: pkg/subpath -> pkg
		parts := strings.SplitN(importPath, "/", 2)
		pkgName = parts[0]
	}

	// Check if this package is in sol-deps.toml
	if !m.isKnownSolidityDep(pkgName) {
		return MappedDep{
			ImportPath: importPath,
			Type:       DependencyUnmapped,
		}
	}

	// Use the package name for the target
	// e.g., "forge-std" -> "soldeps//vendor/forge-std:forge-std"
	// e.g., "@openzeppelin/contracts" -> "soldeps//vendor/@openzeppelin/contracts:contracts"
	targetName := filepath.Base(pkgName)
	target := fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, pkgName, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// isKnownSolidityDep checks if a package is in sol-deps.toml.
func (m *Mapper) isKnownSolidityDep(packageName string) bool {
	if m.config.Solidity == nil || m.config.Solidity.ExternalDeps == nil {
		return false
	}

	return m.config.Solidity.ExternalDeps[packageName]
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
