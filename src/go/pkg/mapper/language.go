package mapper

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"sort"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// TargetKind is what a Buck2 rule builds, as far as sync is concerned.
type TargetKind int

const (
	// NotSynced is a rule of the language whose deps sync leaves alone.
	NotSynced TargetKind = iota
	// Library targets depend on what their sources need.
	Library
	// Binary targets depend on what their sources need, like libraries.
	Binary
	// Test targets depend on what their sources and tests need.
	Test
)

// Language is one language's rules sync plug-in. It owns everything sync
// knows about the language: its rule kinds, the files whose changes make
// its targets stale, and how a package's deps are resolved.
type Language interface {
	// Name identifies the language, e.g. "go".
	Name() string

	// RuleKind reports whether a Buck2 rule kind belongs to the language
	// and, if so, which kind of target it builds.
	RuleKind(rule string) (kind TargetKind, ok bool)

	// DepsAttribute is the attribute of its targets that the resolved
	// deps go in, e.g. "deps".
	DepsAttribute() string

	// SourcePatterns are the file name patterns (filepath.Match) whose
	// changes make the language's targets stale.
	SourcePatterns() []string

	// Dimensions names the configuration dimensions (conditions.OS,
	// conditions.CPU, conditions.GoTag(<tag>)) the deps of the package in
	// pkgDir depend on, or none.
	// Sync resolves the package once per combination of their values, and
	// writes deps that differ as a select().
	Dimensions(pkgDir string) ([]string, error)

	// VariantAttributes names the attributes of a target of kind that
	// select which variant of the package it builds (e.g. Rust features),
	// or none. Sync reads them from the target, evaluating a select() for
	// each configuration, and passes them to ResolveDeps.
	VariantAttributes(kind TargetKind) []string

	// ResolveDeps returns the deps of the package in pkgDir, for one
	// configuration and variant: Deps for its library and binary targets,
	// TestDeps for what its test targets need beyond those, and what
	// couldn't be mapped or isn't synced.
	ResolveDeps(pkgDir string, req Request) (PackageMapping, error)
}

// Request is what one resolution of a package's deps is for.
type Request struct {
	// Config gives a value to each of the language's dimensions for the
	// package; it is empty when the deps don't depend on the configuration.
	Config conditions.Configuration

	// Kind is the kind of target the deps are for.
	Kind TargetKind

	// Variant holds the target's variant attributes (VariantAttributes)
	// that it sets, as they are in the configuration being resolved.
	Variant map[string]starlark.AttributeValue

	// Space is the space Config belongs to, for evaluating the select()s
	// of other targets (e.g. a dependency's variant) in Config.
	Space conditions.Space
}

// unconditional is embedded by a language whose deps depend on neither the
// configuration nor a variant.
type unconditional struct{}

func (unconditional) Dimensions(string) ([]string, error) { return nil, nil }

func (unconditional) VariantAttributes(TargetKind) []string { return nil }

// registry holds the language plug-ins, by the name of the language record
// each serves (nix/buck2/languages.nix). Each is created for the mapper's
// configuration and its language's cell and deps file, and loads the rest
// of its configuration from the project.
var registry = map[string]func(cfg Config, lang syncconfig.Language) Language{
	"go":         newGoLanguage,
	"rust":       newRustLanguage,
	"python":     newPythonLanguage,
	"javascript": newTypeScriptLanguage,
	"solidity":   newSolidityLanguage,
}

// importLanguage is a language whose deps come from the imports its sources
// make, mapped one at a time.
type importLanguage interface {
	Language

	// mapImport maps one import. A DependencyStdLib result is skipped (the
	// standard library, or a module of the same package); a
	// DependencyUnmapped one is reported as unmapped.
	mapImport(imp extraction.Import) MappedDep
}

// mapImports maps imports with lang, dropping skipped ones and duplicate
// targets, and returns the deps sorted by target with the imports that
// couldn't be mapped.
func mapImports(lang importLanguage, imports []extraction.Import) (deps []MappedDep, unmapped []string) {
	seen := make(map[string]bool)
	for _, imp := range imports {
		dep := lang.mapImport(imp)
		switch dep.Type {
		case DependencyStdLib:
			continue
		case DependencyUnmapped:
			unmapped = append(unmapped, imp.Path)
			continue
		}
		if !seen[dep.Target] {
			seen[dep.Target] = true
			deps = append(deps, dep)
		}
	}
	sortDeps(deps)
	return deps, unmapped
}

// mapPackage maps one extracted package's imports and test imports.
func mapPackage(lang importLanguage, pkg extraction.Package) PackageMapping {
	mapping := PackageMapping{Path: pkg.Path}
	mapping.Deps, mapping.UnmappedImports = mapImports(lang, pkg.Imports)
	mapping.TestDeps, mapping.UnmappedTestImports = mapImports(lang, pkg.TestImports)
	return mapping
}

// resolveImports maps an extraction result with lang and merges the
// packages it found: a package dir may hold several (e.g. Solidity's src/
// and test/), all built into the same targets.
func resolveImports(lang importLanguage, result *extraction.Result) PackageMapping {
	var merged PackageMapping
	seenDeps := make(map[string]bool)
	seenTestDeps := make(map[string]bool)
	for _, pkg := range result.Packages {
		m := mapPackage(lang, pkg)
		for _, dep := range m.Deps {
			if !seenDeps[dep.Target] {
				seenDeps[dep.Target] = true
				merged.Deps = append(merged.Deps, dep)
			}
		}
		for _, dep := range m.TestDeps {
			if !seenTestDeps[dep.Target] {
				seenTestDeps[dep.Target] = true
				merged.TestDeps = append(merged.TestDeps, dep)
			}
		}
		merged.UnmappedImports = append(merged.UnmappedImports, m.UnmappedImports...)
		merged.UnmappedTestImports = append(merged.UnmappedTestImports, m.UnmappedTestImports...)
	}
	return merged
}

// runDepsExtract runs the deps-extract tool on pkgDir for a language.
func runDepsExtract(projectRoot, lang, pkgDir string) (*extraction.Result, error) {
	extractorPath := "deps-extract"
	if _, err := exec.LookPath(extractorPath); err != nil {
		return nil, fmt.Errorf("deps-extract not found in PATH (install with: cargo install --path src/rust/deps-extract)")
	}

	cmd := exec.Command(extractorPath, "--lang", lang, pkgDir)
	cmd.Dir = projectRoot

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("deps-extract failed: %s", string(exitErr.Stderr))
		}
		return nil, fmt.Errorf("running deps-extract: %w", err)
	}

	var result extraction.Result
	if err := json.Unmarshal(output, &result); err != nil {
		return nil, fmt.Errorf("parsing deps-extract output: %w", err)
	}
	return &result, nil
}

// resolveWithDepsExtract resolves a package's deps from the imports
// deps-extract finds in it.
func resolveWithDepsExtract(lang importLanguage, projectRoot, pkgDir string) (PackageMapping, error) {
	result, err := runDepsExtract(projectRoot, lang.Name(), pkgDir)
	if err != nil {
		return PackageMapping{}, fmt.Errorf("extractor failed: %w", err)
	}
	return resolveImports(lang, result), nil
}

// sortDeps sorts deps by target, for stable output.
func sortDeps(deps []MappedDep) {
	sort.Slice(deps, func(i, j int) bool {
		return deps[i].Target < deps[j].Target
	})
}

// unmappedDep is the result of mapping an import nothing maps.
func unmappedDep(path string) MappedDep {
	return MappedDep{ImportPath: path, Type: DependencyUnmapped}
}

// skippedDep is the result of mapping an import that isn't a dep.
func skippedDep(path string) MappedDep {
	return MappedDep{ImportPath: path, Type: DependencyStdLib}
}
