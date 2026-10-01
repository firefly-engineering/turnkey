package mapper

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"slices"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/pelletier/go-toml/v2"
	"golang.org/x/mod/modfile"
)

// GoConfig holds Go-specific configuration.
type GoConfig struct {
	// Members are the workspace's modules (ADR 0007), the only first-party
	// Go code: go-deps.toml's [members], or the root go.mod alone when it
	// records none.
	Members []GoMember

	// ExternalCell is the Buck2 cell for external deps (e.g., "godeps").
	ExternalCell string

	// DepsFile is the path to go-deps.toml.
	DepsFile string

	// ExternalDeps maps import paths to their entries from go-deps.toml.
	ExternalDeps map[string]bool
}

// A GoMember is a module of the Go workspace.
type GoMember struct {
	// Path is its module path.
	Path string
	// Dir is its directory, relative to the project root ("." for the
	// root), with forward slashes.
	Dir string
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
	tags, err := goparse.TreeConstraintTags(pkgDir)
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
	ctx, _ := goparse.ConfigContext(req.Config)
	result, err := l.extract(pkgDir, ctx.Environ(), buildTags(req, ctx))
	if err != nil {
		return PackageMapping{}, fmt.Errorf("extractor failed: %w", err)
	}
	return resolveImports(l, result), nil
}

// buildTags returns the build tags a request is resolved with: a binary's
// or test's build_tags, literally, or the tags a library's configuration
// sets (in ctx, its Go build).
func buildTags(req Request, ctx goparse.BuildContext) []string {
	if req.Kind == Binary || req.Kind == Test {
		tags, _ := starlark.Labels(req.Variant["build_tags"])
		return tags
	}
	return ctx.Tags
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
				Kind: classifyGoImport(imp, l.cfg),
			})
		}

		var testImports []extraction.Import
		for _, imp := range append(pkg.TestImports, pkg.XTestImports...) {
			testImports = append(testImports, extraction.Import{
				Path: imp,
				Kind: classifyGoImport(imp, l.cfg),
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

// classifyGoImport determines if an import is stdlib, external, or internal:
// internal when the module that owns it is a workspace member, the longest
// module path prefixing it on a / boundary among the members and the
// modules of go-deps.toml (a member's module path can prefix a third-party
// one's).
func classifyGoImport(imp string, cfg *GoConfig) extraction.ImportKind {
	// Standard library check
	firstSlash := strings.Index(imp, "/")
	firstElement := imp
	if firstSlash > 0 {
		firstElement = imp[:firstSlash]
	}
	if !strings.Contains(firstElement, ".") {
		return extraction.ImportKindStdlib
	}

	if cfg != nil {
		if m, ok := cfg.member(imp); ok && len(m.Path) >= len(cfg.externalModule(imp)) {
			return extraction.ImportKindInternal
		}
	}
	return extraction.ImportKindExternal
}

// within reports whether importPath is in the module modulePath: the module
// path itself, or below it on a / boundary.
func within(importPath, modulePath string) bool {
	return importPath == modulePath || strings.HasPrefix(importPath, modulePath+"/")
}

// member returns the workspace member that owns importPath: the one whose
// module path is its longest prefix, on a / boundary.
func (c *GoConfig) member(importPath string) (GoMember, bool) {
	var owner GoMember
	found := false
	for _, m := range c.Members {
		if within(importPath, m.Path) && (!found || len(m.Path) > len(owner.Path)) {
			owner, found = m, true
		}
	}
	return owner, found
}

// externalModule returns the longest module path of go-deps.toml that
// importPath is in, or "".
func (c *GoConfig) externalModule(importPath string) string {
	owner := ""
	for dep := range c.ExternalDeps {
		if within(importPath, dep) && len(dep) > len(owner) {
			owner = dep
		}
	}
	return owner
}

// detectGoConfig auto-detects Go configuration from the project, with the
// language's cell and deps file.
func detectGoConfig(projectRoot string, lang syncconfig.Language) (*GoConfig, error) {
	cfg := &GoConfig{
		ExternalCell: lang.Cell,
		ExternalDeps: make(map[string]bool),
	}

	// Load go-deps.toml
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, members, err := loadGoDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
		cfg.Members = members
	}

	// Without recorded members, the workspace is the root go.mod alone
	if len(cfg.Members) == 0 {
		if content, err := os.ReadFile(filepath.Join(projectRoot, "go.mod")); err == nil {
			if modulePath := extractModulePath(string(content)); modulePath != "" {
				cfg.Members = []GoMember{{Path: modulePath, Dir: "."}}
			}
		}
	}

	return cfg, nil
}

// extractModulePath extracts the module path from go.mod content, or ""
// when it declares none.
func extractModulePath(content string) string {
	return modfile.ModulePath([]byte(content))
}

// loadGoDeps loads the import paths of the modules in go-deps.toml, and the
// workspace members it records, sorted by directory.
//
// Entries are keyed "path@version" (schema 2), so the import path comes from
// each entry's import_path; a key without one is taken as the path itself.
func loadGoDeps(path string) (map[string]bool, []GoMember, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, nil, err
	}

	var depsFile struct {
		Deps map[string]struct {
			ImportPath string `toml:"import_path"`
		} `toml:"deps"`
		Members map[string]struct {
			Dir string `toml:"dir"`
		} `toml:"members"`
	}
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return nil, nil, err
	}

	result := make(map[string]bool)
	for key, dep := range depsFile.Deps {
		if dep.ImportPath != "" {
			result[dep.ImportPath] = true
		} else {
			result[key] = true
		}
	}
	var members []GoMember
	for modulePath, m := range depsFile.Members {
		members = append(members, GoMember{Path: modulePath, Dir: m.Dir})
	}
	slices.SortFunc(members, func(a, b GoMember) int { return strings.Compare(a.Dir, b.Dir) })
	return result, members, nil
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

// mapInternal maps an internal Go import to the target of its package in
// the member that owns it, named after the import path's last component, as
// buckgen names every Go package (and the godeps cell's forwarding aliases
// name a member's packages).
func (l *goLanguage) mapInternal(importPath string) MappedDep {
	m, ok := l.cfg.member(importPath)
	if !ok {
		return unmappedDep(importPath)
	}

	// e.g. "src/go/pkg/foo" -> "//src/go/pkg/foo:foo", or a member's root
	// package github.com/x/foo in third_party/fork -> "//third_party/fork:foo"
	dir := path.Join(m.Dir, strings.TrimPrefix(importPath[len(m.Path):], "/"))
	if dir == "." {
		dir = ""
	}
	target := fmt.Sprintf("//%s:%s", dir, path.Base(importPath))

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
	return l.cfg.externalModule(importPath) != ""
}

// Manages reports whether rules sync manages the Go rules of the package in
// pkgDir: whether the module it is in, the nearest go.mod above it, is a
// workspace member. Any other go.mod is outside the Go build, as it is for
// go ./... in the workspace (ADR 0007), e.g. a test fixture's.
func (l *goLanguage) Manages(pkgDir string) bool {
	if l.cfg == nil {
		return false
	}
	root, err := filepath.Abs(l.projectRoot)
	if err != nil {
		return false
	}
	abs, err := filepath.Abs(pkgDir)
	if err != nil {
		return false
	}
	rel, err := filepath.Rel(root, abs)
	if err != nil {
		return false
	}
	for dir := filepath.ToSlash(rel); dir != ".." && !strings.HasPrefix(dir, "../"); dir = path.Dir(dir) {
		if _, err := os.Stat(filepath.Join(root, filepath.FromSlash(dir), "go.mod")); err == nil {
			return slices.ContainsFunc(l.cfg.Members, func(m GoMember) bool { return path.Clean(m.Dir) == dir })
		}
		if dir == "." {
			break
		}
	}
	return false
}
