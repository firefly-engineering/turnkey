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

// TypeScriptConfig holds TypeScript/JavaScript-specific configuration.
type TypeScriptConfig struct {
	// ProjectRoot is the TypeScript project root directory.
	ProjectRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "jsdeps").
	ExternalCell string

	// DepsFile is the path to js-deps.toml.
	DepsFile string

	// ExternalDeps holds the names of the npm packages in js-deps.toml.
	ExternalDeps map[string]bool
}

// typescriptRules are the TypeScript and JavaScript rule kinds. Their deps
// go in npm_deps: the rules take the jsdeps cell's packages there, and
// other TypeScript targets in deps, which sync doesn't resolve (it skips
// relative imports).
var typescriptRules = map[string]Rule{
	"typescript_library": {Kind: Library, DepsAttribute: "npm_deps"},
	"typescript_binary":  {Kind: Binary, DepsAttribute: "npm_deps"},
	"typescript_test":    {Kind: Test, DepsAttribute: "npm_deps"},
	"js_library":         {Kind: Library, DepsAttribute: "npm_deps"},
	"js_binary":          {Kind: Binary, DepsAttribute: "npm_deps"},
	"js_test":            {Kind: Test, DepsAttribute: "npm_deps"},
}

// typescriptLanguage resolves a TypeScript or JavaScript package's deps from
// the imports deps-extract finds in its sources.
type typescriptLanguage struct {
	unconditional

	projectRoot string
	cfg         *TypeScriptConfig
}

func newTypeScriptLanguage(mcfg Config, lang syncconfig.Language) Language {
	cfg, _ := detectTypescriptConfig(mcfg.ProjectRoot, lang)
	return &typescriptLanguage{projectRoot: mcfg.ProjectRoot, cfg: cfg}
}

func (l *typescriptLanguage) Name() string { return "typescript" }

func (l *typescriptLanguage) Rule(rule string) (Rule, bool) {
	r, ok := typescriptRules[rule]
	return r, ok
}

func (l *typescriptLanguage) SourcePatterns() []string {
	return []string{"*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"}
}

// ResolveDeps maps the package's imports, and adds for each npm package
// its DefinitelyTyped package (@types/...) when js-deps.toml has one: code
// never imports those, but TypeScript needs them to type-check the import.
func (l *typescriptLanguage) ResolveDeps(pkgDir string, _ Request) (PackageMapping, error) {
	mapping, err := resolveWithDepsExtract(l, l.projectRoot, pkgDir)
	if err != nil {
		return mapping, err
	}
	mapping.Deps = l.withTypes(mapping.Deps)
	mapping.TestDeps = l.withTypes(mapping.TestDeps)
	return mapping, nil
}

// withTypes returns deps plus the @types package of each external one that
// js-deps.toml has, deduplicated and sorted.
func (l *typescriptLanguage) withTypes(deps []MappedDep) []MappedDep {
	if l.cfg == nil {
		return deps
	}
	seen := make(map[string]bool, len(deps))
	for _, dep := range deps {
		seen[dep.Target] = true
	}
	result := deps
	for _, dep := range deps {
		if dep.Type != DependencyExternal {
			continue
		}
		types := typesPackage(npmPackageName(dep.ImportPath))
		if !l.cfg.ExternalDeps[types] {
			continue
		}
		typesDep := MappedDep{
			Target:     l.label(types),
			Type:       DependencyExternal,
			ImportPath: types,
		}
		if !seen[typesDep.Target] {
			seen[typesDep.Target] = true
			result = append(result, typesDep)
		}
	}
	sortDeps(result)
	return result
}

// typesPackage returns the DefinitelyTyped package for an npm package:
// "lodash" -> "@types/lodash", "@org/pkg" -> "@types/org__pkg". A package
// under @types is its own types.
func typesPackage(pkg string) string {
	if strings.HasPrefix(pkg, "@types/") {
		return pkg
	}
	if scoped, ok := strings.CutPrefix(pkg, "@"); ok {
		return "@types/" + strings.Replace(scoped, "/", "__", 1)
	}
	return "@types/" + pkg
}

// detectTypescriptConfig auto-detects TypeScript configuration from the
// project, with the language's cell and deps file.
func detectTypescriptConfig(projectRoot string, lang syncconfig.Language) (*TypeScriptConfig, error) {
	cfg := &TypeScriptConfig{
		ExternalCell: lang.Cell,
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
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, err := loadJSDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadJSDeps loads the npm package names from js-deps.toml, which
// jsdeps-gen writes as a [[package]] array.
func loadJSDeps(path string) (map[string]bool, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

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

	return MappedDep{
		Target:     l.label(pkgName),
		Type:       DependencyExternal,
		ImportPath: modulePath,
	}
}

// label returns the jsdeps cell's target for an npm package: the alias at
// the cell root, named with "@" dropped and "/" replaced by "_" (as
// nix/lib/deps-cell/adapters/javascript.nix names it).
// e.g., "lodash" -> "jsdeps//:lodash", "@types/node" -> "jsdeps//:types_node"
func (l *typescriptLanguage) label(pkg string) string {
	name := strings.ReplaceAll(strings.ReplaceAll(pkg, "@", ""), "/", "_")
	return fmt.Sprintf("%s//:%s", l.cfg.ExternalCell, name)
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
