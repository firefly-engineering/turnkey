package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
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
}

func newPythonLanguage(projectRoot string) Language {
	cfg, _ := detectPythonConfig(projectRoot)
	return &pythonLanguage{projectRoot: projectRoot, cfg: cfg}
}

func (l *pythonLanguage) Name() string { return "python" }

func (l *pythonLanguage) RuleKind(rule string) (TargetKind, bool) {
	kind, ok := pythonRules[rule]
	return kind, ok
}

func (l *pythonLanguage) DepsAttribute() string { return "deps" }

func (l *pythonLanguage) SourcePatterns() []string { return []string{"*.py"} }

func (l *pythonLanguage) ResolveDeps(pkgDir string) (PackageMapping, error) {
	return resolveWithDepsExtract(l, l.projectRoot, pkgDir)
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

	if modules, err := loadUVWorkspaceModules(projectRoot); err == nil {
		cfg.WorkspaceModules = modules
	}

	// Load python-deps.toml
	depsPath := filepath.Join(projectRoot, "python-deps.toml")
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
