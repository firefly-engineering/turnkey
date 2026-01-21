package rules

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// SolidityDepsFile represents the structure of solidity-deps.toml.
type SolidityDepsFile struct {
	Meta     SolidityDepsFileMeta  `toml:"meta"`
	Packages []SolidityDepsPackage `toml:"package"`
}

// SolidityDepsFileMeta contains metadata about the deps file.
type SolidityDepsFileMeta struct {
	Generator string `toml:"generator"`
}

// SolidityDepsPackage represents a single package in solidity-deps.toml.
type SolidityDepsPackage struct {
	Name      string `toml:"name"`
	Version   string `toml:"version"`
	Source    string `toml:"source"` // "npm" or "git"
	URL       string `toml:"url"`
	Integrity string `toml:"integrity,omitempty"`
	Repo      string `toml:"repo,omitempty"`
	Rev       string `toml:"rev,omitempty"`
	Remapping string `toml:"remapping"`
}

// SolidityMapper maps Solidity imports to Buck2 target paths.
type SolidityMapper struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// InternalPrefix is the Buck2 prefix for internal targets.
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps (e.g., "soldeps").
	ExternalCell string

	// ExternalDeps maps package names to their solidity-deps.toml entries.
	ExternalDeps map[string]SolidityDepsPackage

	// Remappings maps import prefixes to their target packages.
	// e.g., "@openzeppelin/contracts/" -> "@openzeppelin/contracts"
	Remappings map[string]string
}

// NewSolidityMapper creates a new Solidity import mapper.
func NewSolidityMapper(projectRoot string) (*SolidityMapper, error) {
	m := &SolidityMapper{
		ProjectRoot:    projectRoot,
		InternalPrefix: "//src/solidity", // Default, can be configured
		ExternalCell:   "soldeps",        // Default, can be configured
		ExternalDeps:   make(map[string]SolidityDepsPackage),
		Remappings:     make(map[string]string),
	}

	// Load solidity-deps.toml
	depsPath := filepath.Join(projectRoot, "solidity-deps.toml")
	if err := m.loadDepsFile(depsPath); err != nil {
		// Not fatal - deps file might not exist yet
	}

	return m, nil
}

// loadDepsFile loads and parses solidity-deps.toml.
func (m *SolidityMapper) loadDepsFile(path string) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	var depsFile SolidityDepsFile
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return err
	}

	// Index by package name and build remappings
	for _, pkg := range depsFile.Packages {
		m.ExternalDeps[pkg.Name] = pkg

		// Parse remapping to understand import prefixes
		// e.g., "@openzeppelin/contracts/=node_modules/@openzeppelin/contracts/"
		if pkg.Remapping != "" {
			parts := strings.SplitN(pkg.Remapping, "=", 2)
			if len(parts) == 2 {
				prefix := strings.TrimSuffix(parts[0], "/")
				m.Remappings[prefix] = pkg.Name
			}
		}
	}

	return nil
}

// MapImport maps a single Solidity import to a Buck2 dependency.
// Returns nil if the import should be ignored.
func (m *SolidityMapper) MapImport(imp Import) *Dependency {
	// Check if it's in solidity-deps.toml (external)
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

// isInternal checks if an import is internal to the project.
func (m *SolidityMapper) isInternal(importPath string) bool {
	return strings.HasPrefix(importPath, "src/")
}

// isExternal checks if an import is in solidity-deps.toml.
func (m *SolidityMapper) isExternal(importPath string) bool {
	// Check direct match
	if _, ok := m.ExternalDeps[importPath]; ok {
		return true
	}

	// Check if any package's remapping matches
	if _, ok := m.Remappings[importPath]; ok {
		return true
	}

	return false
}

// mapInternalImport maps an internal import to a Buck2 target.
func (m *SolidityMapper) mapInternalImport(importPath string) *Dependency {
	// Convert to Buck2 target path
	targetName := strings.ReplaceAll(importPath, "/", "_")
	buckPath := fmt.Sprintf("%s:%s", m.InternalPrefix, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyInternal,
		ImportPath: importPath,
	}
}

// mapExternalImport maps an external import to a Buck2 target.
func (m *SolidityMapper) mapExternalImport(importPath string) *Dependency {
	// Find the package name
	packageName := importPath
	if remapped, ok := m.Remappings[importPath]; ok {
		packageName = remapped
	}

	// Convert package name to Buck2 target name
	// @openzeppelin/contracts -> openzeppelin_contracts
	// forge-std -> forge_std
	targetName := packageNameToSolidityTargetName(packageName)

	// Buck2 target format for Solidity: soldeps//:target_name
	buckPath := fmt.Sprintf("%s//:%s", m.ExternalCell, targetName)

	return &Dependency{
		Target:     buckPath,
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// packageNameToSolidityTargetName converts a Solidity package name to a Buck2 target name.
func packageNameToSolidityTargetName(packageName string) string {
	// Remove @ prefix for scoped packages
	name := strings.TrimPrefix(packageName, "@")
	// Replace / and - with _
	name = strings.ReplaceAll(name, "/", "_")
	name = strings.ReplaceAll(name, "-", "_")
	return name
}

// MapImports maps multiple imports to Buck2 dependencies.
func (m *SolidityMapper) MapImports(imports []Import) (deps []Dependency, unmapped []Import) {
	seen := make(map[string]bool)

	for _, imp := range imports {
		dep := m.MapImport(imp)

		if dep == nil {
			unmapped = append(unmapped, imp)
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
func (m *SolidityMapper) SetInternalPrefix(prefix string) {
	m.InternalPrefix = prefix
}

// SetExternalCell configures the external cell name.
func (m *SolidityMapper) SetExternalCell(cell string) {
	m.ExternalCell = cell
}
