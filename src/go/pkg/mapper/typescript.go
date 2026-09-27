package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

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

// typescriptRules are the TypeScript and JavaScript rule kinds.
var typescriptRules = map[string]TargetKind{
	"typescript_library": Library,
	"typescript_binary":  Binary,
	"typescript_test":    Test,
	"js_library":         Library,
	"js_binary":          Binary,
	"js_test":            Test,
}

// typescriptLanguage resolves a TypeScript or JavaScript package's deps from
// the imports deps-extract finds in its sources.
type typescriptLanguage struct {
	projectRoot string
	cfg         *TypeScriptConfig
}

func newTypeScriptLanguage(projectRoot string) Language {
	cfg, _ := detectTypescriptConfig(projectRoot)
	return &typescriptLanguage{projectRoot: projectRoot, cfg: cfg}
}

func (l *typescriptLanguage) Name() string { return "typescript" }

func (l *typescriptLanguage) RuleKind(rule string) (TargetKind, bool) {
	kind, ok := typescriptRules[rule]
	return kind, ok
}

func (l *typescriptLanguage) SourcePatterns() []string {
	return []string{"*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"}
}

func (l *typescriptLanguage) ResolveDeps(pkgDir string) (PackageMapping, error) {
	return resolveWithDepsExtract(l, l.projectRoot, pkgDir)
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
	if deps, err := loadDepsKeys(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// mapImport maps a single TypeScript import to a Buck2 dependency.
func (l *typescriptLanguage) mapImport(imp extraction.Import) MappedDep {
	if l.cfg == nil {
		return unmappedDep(imp.Path)
	}
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		// Node.js builtins
		return skippedDep(imp.Path)
	case extraction.ImportKindInternal:
		// Relative imports like ./foo or ../bar are files of the same
		// target, not deps
		return skippedDep(imp.Path)
	case extraction.ImportKindExternal:
		return l.mapExternal(imp.Path)
	}
	return unmappedDep(imp.Path)
}

// mapExternal maps an external TypeScript import to a Buck2 target.
func (l *typescriptLanguage) mapExternal(modulePath string) MappedDep {
	pkgName := npmPackageName(modulePath)

	// Check if this package is in js-deps.toml
	if !l.cfg.ExternalDeps[pkgName] {
		return unmappedDep(modulePath)
	}

	// Use the package name for the target
	// e.g., "react" -> "jsdeps//vendor/react:react"
	// e.g., "@types/node" -> "jsdeps//vendor/@types/node:node"
	targetName := filepath.Base(pkgName)
	return MappedDep{
		Target:     fmt.Sprintf("%s//vendor/%s:%s", l.cfg.ExternalCell, pkgName, targetName),
		Type:       DependencyExternal,
		ImportPath: modulePath,
	}
}

// npmPackageName returns the package an import path names, handling
// scoped packages: "@org/pkg/subpath" -> "@org/pkg", "pkg/subpath" -> "pkg".
func npmPackageName(importPath string) string {
	if strings.HasPrefix(importPath, "@") {
		parts := strings.SplitN(importPath, "/", 3)
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
		return importPath
	}
	return strings.SplitN(importPath, "/", 2)[0]
}
