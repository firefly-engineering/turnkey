package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/pelletier/go-toml/v2"
)

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

// solidityRules are the Solidity rule kinds. A solidity_contract's deps are
// not synced.
var solidityRules = map[string]Rule{
	"solidity_library":  {Kind: Library, DepsAttribute: "deps"},
	"solidity_contract": {Kind: NotSynced},
	"solidity_test":     {Kind: Test, DepsAttribute: "deps"},
}

// solidityLanguage resolves a Solidity package's deps from the imports
// deps-extract finds in its sources.
type solidityLanguage struct {
	unconditional

	projectRoot string
	cfg         *SolidityConfig
}

func newSolidityLanguage(mcfg Config, lang syncconfig.Language) Language {
	cfg, _ := detectSolidityConfig(mcfg.ProjectRoot, lang)
	return &solidityLanguage{projectRoot: mcfg.ProjectRoot, cfg: cfg}
}

func (l *solidityLanguage) Name() string { return "solidity" }

func (l *solidityLanguage) Rule(rule string) (Rule, bool) {
	r, ok := solidityRules[rule]
	return r, ok
}

func (l *solidityLanguage) SourcePatterns() []string { return []string{"*.sol"} }

func (l *solidityLanguage) ResolveDeps(pkgDir string, _ Request) (PackageMapping, error) {
	return resolveWithDepsExtract(l, l.projectRoot, pkgDir)
}

// detectSolidityConfig auto-detects Solidity configuration from the
// project, with the language's cell and deps file.
func detectSolidityConfig(projectRoot string, lang syncconfig.Language) (*SolidityConfig, error) {
	cfg := &SolidityConfig{
		ExternalCell: lang.Cell,
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

	// Load solidity-deps.toml
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, err := loadSolidityDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadSolidityDeps loads package names from solidity-deps.toml.
// Handles the [[package]] format used by soldeps-gen.
func loadSolidityDeps(path string) (map[string]bool, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	// solidity-deps.toml uses [[package]] array format, not [deps] map
	var depsFile struct {
		Package []struct {
			Name string `toml:"name"`
		} `toml:"package"`
	}
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return nil, err
	}

	result := make(map[string]bool)
	for _, pkg := range depsFile.Package {
		result[pkg.Name] = true
	}
	return result, nil
}

// mapImport maps a single Solidity import to a Buck2 dependency.
func (l *solidityLanguage) mapImport(imp extraction.Import) MappedDep {
	if l.cfg == nil {
		return unmappedDep(imp.Path)
	}
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		// Solidity has no stdlib
		return skippedDep(imp.Path)
	case extraction.ImportKindInternal:
		// Relative imports like ./Foo.sol or ../Bar.sol are files of the
		// same target, not deps
		return skippedDep(imp.Path)
	case extraction.ImportKindExternal:
		return l.mapExternal(imp.Path)
	}
	return unmappedDep(imp.Path)
}

// mapExternal maps an external Solidity import to a Buck2 target.
func (l *solidityLanguage) mapExternal(importPath string) MappedDep {
	pkgName := npmPackageName(importPath)

	// Check if this package is in solidity-deps.toml
	if !l.cfg.ExternalDeps[pkgName] {
		return unmappedDep(importPath)
	}

	// Convert package name to target name (snake_case at cell root)
	// e.g., "forge-std" -> "soldeps//:forge_std"
	// e.g., "@openzeppelin/contracts" -> "soldeps//:openzeppelin_contracts"
	return MappedDep{
		Target:     fmt.Sprintf("%s//:%s", l.cfg.ExternalCell, solidityPkgToTarget(pkgName)),
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// solidityPkgToTarget converts a Solidity package name to a Buck2 target name.
// "@openzeppelin/contracts" -> "openzeppelin_contracts"
// "forge-std" -> "forge_std"
func solidityPkgToTarget(pkgName string) string {
	// Remove @ prefix
	name := strings.TrimPrefix(pkgName, "@")
	// Replace / and - with _
	name = strings.ReplaceAll(name, "/", "_")
	name = strings.ReplaceAll(name, "-", "_")
	return name
}
