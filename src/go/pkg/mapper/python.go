package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/pelletier/go-toml/v2"
)

// PythonConfig holds Python-specific configuration.
type PythonConfig struct {
	// ProjectRoot is the Python project root directory.
	ProjectRoot string

	// WorkspaceModules maps the packages the uv workspace's members provide,
	// as dotted module names, to the member's directory relative to the
	// project root: {"turnkey.cfg": "src/python/cfg"}.
	WorkspaceModules map[string]string

	// ExternalCell is the Buck2 cell for external deps (e.g., "pydeps").
	ExternalCell string

	// DepsFile is the path to python-deps.toml.
	DepsFile string

	// ExternalDeps maps package names to their entries from python-deps.toml.
	ExternalDeps map[string]bool
}

// pythonRules are the Python rule kinds.
var pythonRules = map[string]TargetKind{
	"python_library": Library,
	"python_binary":  Binary,
	"python_test":    Test,
}

// pythonLanguage resolves a Python package's deps from the imports
// deps-extract finds in its sources.
type pythonLanguage struct {
	projectRoot string
	cfg         *PythonConfig

	// members are the uv workspace's members, with their declared deps
	members []*pyMember

	// extracted caches deps-extract's mapping of each package dir
	extracted map[string]PackageMapping

	// version is the Python toolchain's version, once looked up
	version *string
}

func newPythonLanguage(mcfg Config, lang syncconfig.Language) Language {
	projectRoot := mcfg.ProjectRoot
	cfg, _ := detectPythonConfig(projectRoot, lang)
	l := &pythonLanguage{projectRoot: projectRoot, cfg: cfg, extracted: make(map[string]PackageMapping)}
	if cfg != nil {
		l.members, _ = loadPyMembers(projectRoot)
	}
	return l
}

// Dimensions: a dependency's platform marker (sys_platform, ...) makes the
// deps depend on the platform.
func (l *pythonLanguage) Dimensions(string) ([]string, error) {
	return []string{conditions.OS, conditions.CPU}, nil
}

// VariantAttributes: a Python target builds its package with extras.
func (l *pythonLanguage) VariantAttributes(TargetKind) []string {
	return []string{"extras"}
}

func (l *pythonLanguage) Name() string { return "python" }

func (l *pythonLanguage) RuleKind(rule string) (TargetKind, bool) {
	kind, ok := pythonRules[rule]
	return kind, ok
}

func (l *pythonLanguage) DepsAttribute() string { return "deps" }

func (l *pythonLanguage) SourcePatterns() []string { return []string{"*.py"} }

// ResolveDeps maps the imports deps-extract finds in pkgDir; in a uv
// workspace member, it then applies the markers and extras its
// pyproject.toml declares (applyMarkers) for the configuration.
func (l *pythonLanguage) ResolveDeps(pkgDir string, req Request) (PackageMapping, error) {
	mapping, ok := l.extracted[pkgDir]
	if !ok {
		var err error
		mapping, err = resolveWithDepsExtract(l, l.projectRoot, pkgDir)
		if err != nil {
			return mapping, err
		}
		l.extracted[pkgDir] = mapping
	}
	rel, err := filepath.Rel(l.projectRoot, pkgDir)
	if err != nil {
		return mapping, nil
	}
	member := l.memberOf(filepath.ToSlash(rel))
	if member == nil {
		return mapping, nil
	}
	mapping.Deps = append([]MappedDep(nil), mapping.Deps...)
	mapping.TestDeps = append([]MappedDep(nil), mapping.TestDeps...)
	mapping.UnmappedImports = append([]string(nil), mapping.UnmappedImports...)
	return l.applyMarkers(mapping, member, req), nil
}

// detectPythonConfig auto-detects Python configuration from the project,
// with the language's cell and deps file.
func detectPythonConfig(projectRoot string, lang syncconfig.Language) (*PythonConfig, error) {
	cfg := &PythonConfig{
		ExternalCell: lang.Cell,
		ExternalDeps: make(map[string]bool),
	}

	// Check for pyproject.toml
	pyprojectPath := filepath.Join(projectRoot, "pyproject.toml")
	if _, err := os.Stat(pyprojectPath); err != nil {
		return nil, fmt.Errorf("no pyproject.toml found")
	}
	cfg.ProjectRoot = projectRoot

	if modules, err := loadUVWorkspaceModules(projectRoot); err == nil {
		cfg.WorkspaceModules = modules
	}

	// Load python-deps.toml
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, err := loadDepsKeys(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadDepsKeys loads the keys of a deps file's [deps] table: the package
// names of python-deps.toml.
func loadDepsKeys(path string) (map[string]bool, error) {
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

// mapImport maps a single Python import to a Buck2 dependency.
func (l *pythonLanguage) mapImport(imp extraction.Import) MappedDep {
	if l.cfg == nil {
		return unmappedDep(imp.Path)
	}
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return skippedDep(imp.Path)
	case extraction.ImportKindInternal:
		// Relative imports (.module, ..module) are modules of the same
		// target, and absolute internal imports can't be resolved without
		// more context: neither is a dep.
		return skippedDep(imp.Path)
	case extraction.ImportKindExternal:
		return l.mapExternal(imp.Path)
	}
	return unmappedDep(imp.Path)
}

// mapExternal maps an external Python import to a Buck2 target.
func (l *pythonLanguage) mapExternal(modulePath string) MappedDep {
	cfg := l.cfg

	// A package of a uv workspace member maps to the member's target:
	// "turnkey.cargo.toml" -> "//src/python/cargo:cargo"
	if dir, ok := workspaceModuleDir(cfg.WorkspaceModules, modulePath); ok {
		return MappedDep{
			Target:     fmt.Sprintf("//%s:%s", dir, filepath.Base(dir)),
			Type:       DependencyInternal,
			ImportPath: modulePath,
		}
	}

	// Get top-level package name
	topLevel := modulePath
	if idx := strings.Index(modulePath, "."); idx > 0 {
		topLevel = modulePath[:idx]
	}

	// Check if this package is in python-deps.toml
	if !cfg.ExternalDeps[topLevel] {
		return unmappedDep(modulePath)
	}

	// Use the top-level package name for the target
	// e.g., "requests" -> "pydeps//vendor/requests:requests"
	return MappedDep{
		Target:     fmt.Sprintf("%s//vendor/%s:%s", cfg.ExternalCell, topLevel, topLevel),
		Type:       DependencyExternal,
		ImportPath: modulePath,
	}
}
