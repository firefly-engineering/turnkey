package mapper

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/pelletier/go-toml/v2"
	"golang.org/x/mod/modfile"
)

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

// goRules are the Go rule kinds. A binary or a test is built with its
// build_tags, literally; a library gets its tags from the configuration.
var goRules = map[string]Rule{
	"go_library":          {Kind: Library, DepsAttribute: "deps"},
	"go_exported_library": {Kind: Library, DepsAttribute: "deps"},
	"go_binary":           {Kind: Binary, DepsAttribute: "deps", Variant: []string{"build_tags"}},
	"go_test":             {Kind: Test, DepsAttribute: "deps", Variant: []string{"build_tags"}},
}

// goLanguage resolves a Go package's deps from the imports go list reports,
// on each platform: for a library, with each combination of the allowed
// build tags its build constraints use; for a binary or a test, with its
// build_tags.
type goLanguage struct {
	projectRoot string
	cfg         *GoConfig

	// allowedTags are the build tags a library's deps may vary with
	// (sync.toml's [conditions] go_tags)
	allowedTags []string
}

func newGoLanguage(mcfg Config, lang syncconfig.Language) Language {
	cfg, _ := detectGoConfig(mcfg.ProjectRoot, lang)
	return &goLanguage{projectRoot: mcfg.ProjectRoot, cfg: cfg, allowedTags: mcfg.Conditions.GoTags}
}

// Dimensions: the platform, and each allowed build tag the package's build
// constraints use.
func (l *goLanguage) Dimensions(pkgDir string) (Dimensions, error) {
	dims := Dimensions{Platform: platform.Platform}
	tags, err := constraintTags(pkgDir)
	if err != nil {
		return Dimensions{}, err
	}
	for _, tag := range tags {
		if slices.Contains(l.allowedTags, tag) {
			dims.OnOff = append(dims.OnOff, goparse.TagDimension(tag))
		}
	}
	return dims, nil
}

func (l *goLanguage) Name() string { return "go" }

func (l *goLanguage) Rule(rule string) (Rule, bool) {
	r, ok := goRules[rule]
	return r, ok
}

func (l *goLanguage) SourcePatterns() []string { return []string{"*.go"} }

func (l *goLanguage) ResolveDeps(pkgDir string, req Request) (PackageMapping, error) {
	result, err := l.extract(pkgDir, goEnv(req.Config), buildTags(req))
	if err != nil {
		return PackageMapping{}, fmt.Errorf("extractor failed: %w", err)
	}
	return resolveImports(l, result), nil
}

// goEnv returns the environment go list needs to see a configuration's
// platform, whatever the host: GOOS, GOARCH and cgo. A configuration
// without a platform gets the host's.
func goEnv(config conditions.Configuration) []string {
	goos := map[string]string{"linux": "linux", "macos": "darwin"}[config[conditions.OS]]
	goarch := map[string]string{"x86_64": "amd64", "arm64": "arm64"}[config[conditions.CPU]]
	if goos == "" || goarch == "" {
		return nil
	}
	return []string{"GOOS=" + goos, "GOARCH=" + goarch, "CGO_ENABLED=1"}
}

// buildTags returns the build tags a request is resolved with: a binary's
// or test's build_tags, literally, or the tags a library's configuration
// sets.
func buildTags(req Request) []string {
	if req.Kind == Binary || req.Kind == Test {
		tags, _ := starlark.Labels(req.Variant["build_tags"])
		return tags
	}
	return goparse.ConfigTags(req.Config)
}

// extract lists the imports of the Go packages under pkgDir with go list,
// in the environment env (added to the process's) and with tags.
func (l *goLanguage) extract(pkgDir string, env, tags []string) (*extraction.Result, error) {
	result := extraction.NewResult("go")

	args := []string{"list", "-e", "-json"}
	if len(tags) > 0 {
		args = append(args, "-tags", strings.Join(tags, ","))
	}
	cmd := exec.Command("go", append(args, "./...")...)
	cmd.Dir = pkgDir
	if len(env) > 0 {
		cmd.Env = append(os.Environ(), env...)
	}

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			result.AddError(fmt.Sprintf("go list warning: %s", string(exitErr.Stderr)))
		} else {
			return nil, fmt.Errorf("running go list: %w", err)
		}
	}

	modulePath := ""
	if l.cfg != nil {
		modulePath = l.cfg.ModulePath
	}

	// Parse JSON stream
	dec := json.NewDecoder(strings.NewReader(string(output)))
	for dec.More() {
		var pkg struct {
			Dir          string
			ImportPath   string
			GoFiles      []string
			TestGoFiles  []string
			Imports      []string
			TestImports  []string
			XTestImports []string
		}
		if err := dec.Decode(&pkg); err != nil {
			continue
		}

		// Calculate relative path
		relPath, err := filepath.Rel(l.projectRoot, pkg.Dir)
		if err != nil {
			relPath = pkg.Dir
		}

		// Classify imports
		var imports []extraction.Import
		for _, imp := range pkg.Imports {
			imports = append(imports, extraction.Import{
				Path: imp,
				Kind: classifyGoImport(imp, modulePath),
			})
		}

		var testImports []extraction.Import
		for _, imp := range append(pkg.TestImports, pkg.XTestImports...) {
			testImports = append(testImports, extraction.Import{
				Path: imp,
				Kind: classifyGoImport(imp, modulePath),
			})
		}

		result.AddPackage(extraction.Package{
			Path:        relPath,
			Files:       pkg.GoFiles,
			Imports:     imports,
			TestImports: testImports,
		})
	}

	return result, nil
}

// classifyGoImport determines if an import is stdlib, external, or internal.
func classifyGoImport(imp, modulePath string) extraction.ImportKind {
	// Standard library check
	firstSlash := strings.Index(imp, "/")
	firstElement := imp
	if firstSlash > 0 {
		firstElement = imp[:firstSlash]
	}
	if !strings.Contains(firstElement, ".") {
		return extraction.ImportKindStdlib
	}

	// Internal check
	if modulePath != "" && strings.HasPrefix(imp, modulePath) {
		return extraction.ImportKindInternal
	}

	return extraction.ImportKindExternal
}

// detectGoConfig auto-detects Go configuration from the project, with the
// language's cell and deps file.
func detectGoConfig(projectRoot string, lang syncconfig.Language) (*GoConfig, error) {
	cfg := &GoConfig{
		ExternalCell: lang.Cell,
		ExternalDeps: make(map[string]bool),
	}

	// Read module path from go.mod
	modPath := filepath.Join(projectRoot, "go.mod")
	if content, err := os.ReadFile(modPath); err == nil {
		cfg.ModulePath = extractModulePath(string(content))
	}

	// Load go-deps.toml
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, err := loadGoDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// extractModulePath extracts the module path from go.mod content, or ""
// when it declares none.
func extractModulePath(content string) string {
	return modfile.ModulePath([]byte(content))
}

// loadGoDeps loads the import paths of the modules in go-deps.toml.
//
// Entries are keyed "path@version" (schema 2), so the import path comes from
// each entry's import_path; a key without one is taken as the path itself.
func loadGoDeps(path string) (map[string]bool, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var depsFile struct {
		Deps map[string]struct {
			ImportPath string `toml:"import_path"`
		} `toml:"deps"`
	}
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return nil, err
	}

	result := make(map[string]bool)
	for key, dep := range depsFile.Deps {
		if dep.ImportPath != "" {
			result[dep.ImportPath] = true
		} else {
			result[key] = true
		}
	}
	return result, nil
}

// mapImport maps a single Go import to a Buck2 dependency.
func (l *goLanguage) mapImport(imp extraction.Import) MappedDep {
	if l.cfg == nil {
		return unmappedDep(imp.Path)
	}
	switch imp.Kind {
	case extraction.ImportKindStdlib:
		return skippedDep(imp.Path)
	case extraction.ImportKindInternal:
		return l.mapInternal(imp.Path)
	case extraction.ImportKindExternal:
		return l.mapExternal(imp.Path)
	}
	return unmappedDep(imp.Path)
}

// mapInternal maps an internal Go import to a Buck2 target.
func (l *goLanguage) mapInternal(importPath string) MappedDep {
	// Remove module path prefix to get relative path
	relPath := strings.TrimPrefix(importPath, l.cfg.ModulePath+"/")

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

// mapExternal maps an external Go import to a Buck2 target.
func (l *goLanguage) mapExternal(importPath string) MappedDep {
	// Check if this import or a parent is in go-deps.toml
	if !l.isKnownDep(importPath) {
		return unmappedDep(importPath)
	}

	// Use the full import path for the target
	// e.g., "golang.org/x/sys/cpu" -> "godeps//vendor/golang.org/x/sys/cpu:cpu"
	targetName := filepath.Base(importPath)
	target := fmt.Sprintf("%s//vendor/%s:%s", l.cfg.ExternalCell, importPath, targetName)

	return MappedDep{
		Target:     target,
		Type:       DependencyExternal,
		ImportPath: importPath,
	}
}

// isKnownDep checks if an import is in go-deps.toml or is a subpackage.
func (l *goLanguage) isKnownDep(importPath string) bool {
	if l.cfg.ExternalDeps == nil {
		return false
	}

	// Check exact match
	if l.cfg.ExternalDeps[importPath] {
		return true
	}

	// Check if any registered dep is a prefix
	for dep := range l.cfg.ExternalDeps {
		if strings.HasPrefix(importPath, dep+"/") {
			return true
		}
	}

	return false
}
