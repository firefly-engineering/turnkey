// Package mapper resolves a package's dependencies to Buck2 target
// references, through one plug-in per language (see Language), and applies
// them to rules.star files using the starlark object model.
package mapper

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
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

	// Languages are the languages to resolve deps for, in order, each with
	// the cell and deps file of its external deps (sync.toml's
	// [[languages]]).
	Languages []syncconfig.Language

	// Conditions are the build configurations sync evaluates; Go's
	// allowed build tags come from them.
	Conditions syncconfig.ConditionsConfig
}

// Mapper resolves packages' deps to Buck2 targets through the registered
// language plug-ins.
type Mapper struct {
	config    Config
	languages []Language
}

// New creates a Mapper with the plug-in of each of the configured
// languages. A language without a plug-in is an error.
func New(cfg Config) (*Mapper, error) {
	m := &Mapper{config: cfg}
	for _, lang := range cfg.Languages {
		newLanguage, ok := registry[lang.Name]
		if !ok {
			return nil, fmt.Errorf("no rules sync plug-in for language %q", lang.Name)
		}
		m.languages = append(m.languages, newLanguage(cfg, lang))
	}
	return m, nil
}

// NewWith creates a Mapper with the given language plug-ins instead of the
// registered ones.
func NewWith(cfg Config, languages ...Language) *Mapper {
	return &Mapper{config: cfg, languages: languages}
}

// Languages returns the language plug-ins, in registration order.
func (m *Mapper) Languages() []Language {
	return m.languages
}

// Language returns the plug-in named name, or nil.
func (m *Mapper) Language(name string) Language {
	for _, lang := range m.languages {
		if lang.Name() == name {
			return lang
		}
	}
	return nil
}

// RuleLanguage returns the plug-in a Buck2 rule kind belongs to, and the
// kind of target it builds, or nil.
func (m *Mapper) RuleLanguage(rule string) (Language, TargetKind) {
	for _, lang := range m.languages {
		if kind, ok := lang.RuleKind(rule); ok {
			return lang, kind
		}
	}
	return nil, NotSynced
}

// MapExtractionResult converts an extraction result to mapped dependencies,
// with the plug-in of the result's language. A language whose deps don't
// come from imports, or no language, leaves every import unmapped.
func (m *Mapper) MapExtractionResult(result *extraction.Result) (map[string]PackageMapping, error) {
	lang, _ := m.Language(result.Language).(importLanguage)

	mappings := make(map[string]PackageMapping)
	for _, pkg := range result.Packages {
		if lang != nil {
			mappings[pkg.Path] = mapPackage(lang, pkg)
			continue
		}
		mapping := PackageMapping{Path: pkg.Path}
		for _, imp := range pkg.Imports {
			mapping.UnmappedImports = append(mapping.UnmappedImports, imp.Path)
		}
		for _, imp := range pkg.TestImports {
			mapping.UnmappedTestImports = append(mapping.UnmappedTestImports, imp.Path)
		}
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

	// UnmappedImports are the package's non-test imports that couldn't be
	// mapped. They leave the deps of every target built from the package
	// incomplete.
	UnmappedImports []string

	// UnmappedTestImports are test-only imports that couldn't be mapped.
	// They leave only the test target's deps incomplete.
	UnmappedTestImports []string

	// UnsyncedDeps are declared deps sync doesn't manage (e.g. a Rust
	// crate's build dependencies): they are neither added nor removed.
	UnsyncedDeps []UnsyncedDep

	// Attrs are other attributes of the package's targets that sync owns,
	// with their values: e.g. a Rust target's "features". An attribute in
	// Attrs is set to exactly its value; one with no values isn't added.
	Attrs map[string][]string
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
