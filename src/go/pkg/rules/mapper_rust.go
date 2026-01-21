package rules

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// RustDepsFile represents the structure of rust-deps.toml.
type RustDepsFile struct {
	SchemaVersion int                      `toml:"schema_version"`
	Deps          map[string]RustDepsEntry `toml:"deps"`
}

// RustDepsEntry represents a single dependency in rust-deps.toml.
type RustDepsEntry struct {
	Name    string `toml:"name"`
	Version string `toml:"version"`
	Hash    string `toml:"hash"`
}

// RustMapper maps Rust crate imports to Buck2 target paths.
type RustMapper struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// InternalPrefix is the Buck2 prefix for internal targets (e.g., "//src/rust").
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps (e.g., "rustdeps").
	ExternalCell string

	// ExternalDeps maps crate names to their rust-deps.toml entries.
	ExternalDeps map[string]RustDepsEntry
}

// NewRustMapper creates a new Rust crate mapper.
func NewRustMapper(projectRoot string) (*RustMapper, error) {
	m := &RustMapper{
		ProjectRoot:    projectRoot,
		InternalPrefix: "//src/rust",  // Default, can be configured
		ExternalCell:   "rustdeps",    // Default, can be configured
		ExternalDeps:   make(map[string]RustDepsEntry),
	}

	// Load rust-deps.toml
	depsPath := filepath.Join(projectRoot, "rust-deps.toml")
	if err := m.loadDepsFile(depsPath); err != nil {
		// Not fatal - deps file might not exist yet
	}

	return m, nil
}

// loadDepsFile loads and parses rust-deps.toml.
func (m *RustMapper) loadDepsFile(path string) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var depsFile RustDepsFile
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return err
	}

	// Index by crate name (without version suffix)
	for key, entry := range depsFile.Deps {
		// Keys in rust-deps.toml are like "crate-name@version"
		// We want to index by just the crate name
		crateName := entry.Name
		if crateName == "" {
			// Fallback: extract from key
			crateName = strings.Split(key, "@")[0]
		}
		// Normalize crate names: Rust allows - but code uses _
		normalized := strings.ReplaceAll(crateName, "-", "_")
		m.ExternalDeps[normalized] = entry
		// Also store with original name
		m.ExternalDeps[crateName] = entry
	}

	return nil
}

// MapImport maps a single Rust crate import to a Buck2 dependency.
// Returns nil if the import should be ignored (stdlib, etc.).
func (m *RustMapper) MapImport(imp Import) *Dependency {
	// Skip standard library
	if m.isStdLib(imp.Path) {
		return &Dependency{
			ImportPath: imp.Path,
			Type:       DependencyStdLib,
		}
	}

	// Check if it's in rust-deps.toml (external)
	if m.isExternal(imp.Path) {
		return m.mapExternalImport(imp.Path)
	}

	// Check if it's internal (workspace member)
	if m.isInternal(imp.Path) {
		return m.mapInternalImport(imp.Path)
	}

	// Unknown import - might be a missing dependency
	return nil
}

// isStdLib checks if a crate is part of the Rust standard library.
func (m *RustMapper) isStdLib(crateName string) bool {
	stdLibCrates := map[string]bool{
		"std":         true,
		"core":        true,
		"alloc":       true,
		"collections": true,
		"test":        true,
		"proc_macro":  true,
		"self":        true,
		"crate":       true,
		"super":       true,
	}
	return stdLibCrates[crateName]
}

// isInternal checks if a crate is internal to the monorepo.
func (m *RustMapper) isInternal(crateName string) bool {
	// Check if it's a workspace member by looking for Cargo.toml
	// This is a simplified check - could be enhanced with actual workspace parsing
	return false
}

// isExternal checks if a crate is in rust-deps.toml.
func (m *RustMapper) isExternal(crateName string) bool {
	// Normalize the crate name
	normalized := strings.ReplaceAll(crateName, "-", "_")
	_, ok := m.ExternalDeps[normalized]
	if !ok {
		_, ok = m.ExternalDeps[crateName]
	}
	return ok
}

// mapInternalImport maps an internal crate to a Buck2 target.
func (m *RustMapper) mapInternalImport(crateName string) *Dependency {
	// Convert to Buck2 target path
	buckPath := fmt.Sprintf("%s/%s:%s", m.InternalPrefix, crateName, crateName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyInternal,
		ImportPath: crateName,
	}
}

// mapExternalImport maps an external crate to a Buck2 target.
func (m *RustMapper) mapExternalImport(crateName string) *Dependency {
	// Normalize crate name for lookup
	normalized := strings.ReplaceAll(crateName, "-", "_")

	entry, ok := m.ExternalDeps[normalized]
	if !ok {
		entry = m.ExternalDeps[crateName]
	}

	// Use the original crate name from the entry if available
	targetName := entry.Name
	if targetName == "" {
		targetName = crateName
	}

	// Buck2 target format: rustdeps//vendor/{crate_name}:{crate_name}
	buckPath := fmt.Sprintf("%s//vendor/%s:%s", m.ExternalCell, targetName, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyExternal,
		ImportPath: crateName,
	}
}

// MapImports maps multiple imports to Buck2 dependencies.
func (m *RustMapper) MapImports(imports []Import) (deps []Dependency, unmapped []Import) {
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
func (m *RustMapper) SetInternalPrefix(prefix string) {
	m.InternalPrefix = prefix
}

// SetExternalCell configures the external cell name.
func (m *RustMapper) SetExternalCell(cell string) {
	m.ExternalCell = cell
}
