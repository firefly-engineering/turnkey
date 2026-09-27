package mapper

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"sort"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
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

	// ResolveDeps returns the deps of the package in pkgDir: Deps for its
	// library and binary targets, TestDeps for what its test targets need
	// beyond those, and what couldn't be mapped or isn't synced.
	ResolveDeps(pkgDir string) (PackageMapping, error)
}

// registry holds the language plug-ins, in the order sync tries them. Each
// is created for a project root and loads its own configuration from it.
var registry = []func(projectRoot string) Language{
	newGoLanguage,
	newRustLanguage,
	newPythonLanguage,
	newTypeScriptLanguage,
	newSolidityLanguage,
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
