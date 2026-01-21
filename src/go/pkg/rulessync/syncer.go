// Package rulessync orchestrates rules.star synchronization using the new architecture:
// - Extractors for language-specific import detection
// - Mapper for converting imports to Buck2 targets
// - Starlark object model for reading/writing rules.star
package rulessync

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/mapper"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// Config holds syncer configuration.
type Config struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// GoExtractorPath is the path to the go-deps-extract binary.
	// If empty, uses "go-deps-extract" from PATH.
	GoExtractorPath string

	// DryRun if true, doesn't write changes.
	DryRun bool

	// Verbose enables verbose output.
	Verbose bool

	// Force if true, syncs all files even if they appear up-to-date.
	// When false, uses mtime-based staleness detection to skip files.
	Force bool
}

// Syncer orchestrates rules.star synchronization.
type Syncer struct {
	config Config
	mapper *mapper.Mapper
}

// NewSyncer creates a new Syncer.
func NewSyncer(cfg Config) (*Syncer, error) {
	m, err := mapper.New(mapper.Config{
		ProjectRoot: cfg.ProjectRoot,
	})
	if err != nil {
		return nil, fmt.Errorf("creating mapper: %w", err)
	}

	return &Syncer{
		config: cfg,
		mapper: m,
	}, nil
}

// SyncResult contains the result of syncing a single rules.star file.
type SyncResult struct {
	// Path is the path to the rules.star file.
	Path string

	// Updated is true if the file was modified.
	Updated bool

	// Skipped is true if the file was skipped due to staleness check.
	Skipped bool

	// Added lists dependencies that were added.
	Added []string

	// Removed lists dependencies that were removed.
	Removed []string

	// Errors contains any errors encountered.
	Errors []string
}

// SyncDirectory syncs all rules.star files in a directory tree.
func (s *Syncer) SyncDirectory(dir string) ([]SyncResult, error) {
	// Find all Go packages with rules.star
	var results []SyncResult

	err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip vendor and hidden directories
		if info.IsDir() {
			name := info.Name()
			if name == "vendor" || name == "testdata" || strings.HasPrefix(name, ".") {
				return filepath.SkipDir
			}
			return nil
		}

		// Only process rules.star files
		if info.Name() != "rules.star" {
			return nil
		}

		result, err := s.SyncFile(path)
		if err != nil {
			results = append(results, SyncResult{
				Path:   path,
				Errors: []string{err.Error()},
			})
		} else {
			results = append(results, *result)
		}

		return nil
	})

	return results, err
}

// SyncFile syncs a single rules.star file.
func (s *Syncer) SyncFile(rulesPath string) (*SyncResult, error) {
	result := &SyncResult{Path: rulesPath}

	// Determine package directory
	pkgDir := filepath.Dir(rulesPath)

	// Parse the rules.star file to detect language
	f, err := starlark.ParseFile(rulesPath)
	if err != nil {
		return nil, fmt.Errorf("parsing rules.star: %w", err)
	}

	// Detect language from rules.star content
	language := s.detectLanguage(f)
	if language == "" {
		// Can't determine language, skip
		return result, nil
	}

	// Check staleness before running extractor (unless Force mode)
	if !s.config.Force {
		stale, err := s.isStale(rulesPath, pkgDir, language)
		if err != nil {
			// On error, assume stale to be safe
			if s.config.Verbose {
				fmt.Fprintf(os.Stderr, "  staleness check failed for %s: %v\n", rulesPath, err)
			}
		} else if !stale {
			result.Skipped = true
			return result, nil
		}
	}

	// Run extractor for this package
	extractResult, err := s.runExtractor(language, pkgDir)
	if err != nil {
		result.Errors = append(result.Errors, fmt.Sprintf("extractor failed: %v", err))
		return result, nil
	}

	// Map extraction results to Buck2 targets
	mappings, err := s.mapper.MapExtractionResult(extractResult)
	if err != nil {
		result.Errors = append(result.Errors, fmt.Sprintf("mapping failed: %v", err))
		return result, nil
	}

	// Merge all package mappings
	// For languages with subdirectories (like Solidity with src/ and test/),
	// combine deps from all packages
	pkgMapping := mergePackageMappings(mappings)

	// Filter out self-references (deps pointing to the current package)
	// e.g., when syncing src/python/cargo, filter out //src/python/cargo:cargo
	selfTarget := computeSelfTarget(pkgDir, s.config.ProjectRoot)
	pkgMapping.Deps = filterSelfReference(pkgMapping.Deps, selfTarget)
	pkgMapping.TestDeps = filterSelfReference(pkgMapping.TestDeps, selfTarget)

	// Report unmapped imports
	for _, unmapped := range pkgMapping.UnmappedImports {
		result.Errors = append(result.Errors, fmt.Sprintf("unmapped import: %s", unmapped))
	}

	// Apply changes to targets
	modified := false

	// Process library targets
	for _, target := range f.Targets {
		if isLibraryTarget(target.Rule) {
			oldDeps := target.GetDeps()
			newDeps := mapper.DepsToTargets(pkgMapping.Deps)

			// Preserve manual deps (outside auto-managed section)
			newDeps = mergeWithPreserved(oldDeps, newDeps)

			if !stringSlicesEqual(oldDeps, newDeps) {
				target.SetDeps(newDeps)
				modified = true
				result.Added, result.Removed = diffDeps(oldDeps, newDeps)
			}
		}

		if isTestTarget(target.Rule) {
			oldDeps := target.GetDeps()

			// Check if test has target_under_test or local target deps (":foo")
			// If so, library deps come transitively - don't add them directly
			hasTargetUnderTest := target.GetStringAttr("target_under_test") != ""
			hasLocalTargetDep := hasLocalDep(oldDeps)

			var newDeps []string
			seen := make(map[string]bool)

			if !hasTargetUnderTest && !hasLocalTargetDep {
				// No target_under_test and no local deps, so include library deps
				for _, d := range mapper.DepsToTargets(pkgMapping.Deps) {
					if !seen[d] {
						seen[d] = true
						newDeps = append(newDeps, d)
					}
				}
			}

			// Always add test-only deps
			for _, d := range mapper.DepsToTargets(pkgMapping.TestDeps) {
				if !seen[d] {
					seen[d] = true
					newDeps = append(newDeps, d)
				}
			}

			// Preserve manual deps
			newDeps = mergeWithPreserved(oldDeps, newDeps)

			if !stringSlicesEqual(oldDeps, newDeps) {
				target.SetDeps(newDeps)
				modified = true
				// Note: we're only tracking library target changes in Added/Removed
			}
		}
	}

	// Write if modified
	if modified && !s.config.DryRun {
		output := f.Write()
		if err := os.WriteFile(rulesPath, output, 0644); err != nil {
			return nil, fmt.Errorf("writing rules.star: %w", err)
		}
	}

	result.Updated = modified
	return result, nil
}

// isStale checks if rules.star needs updating based on source file mtimes.
// Returns true if any source file is newer than rules.star.
func (s *Syncer) isStale(rulesPath, pkgDir, language string) (bool, error) {
	// Get rules.star mtime
	rulesInfo, err := os.Stat(rulesPath)
	if err != nil {
		return true, err // If we can't stat rules.star, assume stale
	}
	rulesMtime := rulesInfo.ModTime()

	// Get source file patterns for this language
	patterns := sourcePatterns(language)
	if len(patterns) == 0 {
		return true, nil // Unknown language, assume stale
	}

	// Walk the directory and check mtimes
	var newerCount int
	err = filepath.Walk(pkgDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip hidden directories and common non-source directories
		if info.IsDir() {
			name := info.Name()
			if name == "vendor" || name == "node_modules" || name == "testdata" ||
				name == "__pycache__" || name == ".venv" || name == "target" ||
				strings.HasPrefix(name, ".") {
				return filepath.SkipDir
			}
			return nil
		}

		// Check if file matches any source pattern
		for _, pattern := range patterns {
			matched, _ := filepath.Match(pattern, info.Name())
			if matched {
				if info.ModTime().After(rulesMtime) {
					newerCount++
					// Found a newer file, we're done
					return filepath.SkipAll
				}
				break
			}
		}
		return nil
	})

	if err != nil && err != filepath.SkipAll {
		return true, err
	}

	return newerCount > 0, nil
}

// sourcePatterns returns file patterns for source files in a given language.
func sourcePatterns(language string) []string {
	switch language {
	case "go":
		return []string{"*.go"}
	case "rust":
		return []string{"*.rs", "Cargo.toml"}
	case "python":
		return []string{"*.py"}
	case "typescript":
		return []string{"*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"}
	case "solidity":
		return []string{"*.sol"}
	default:
		return nil
	}
}

// detectLanguage determines the language from rules.star content.
func (s *Syncer) detectLanguage(f *starlark.File) string {
	for _, target := range f.Targets {
		switch {
		case strings.HasPrefix(target.Rule, "go_"):
			return "go"
		case strings.HasPrefix(target.Rule, "rust_"):
			return "rust"
		case strings.HasPrefix(target.Rule, "python_"):
			return "python"
		case strings.HasPrefix(target.Rule, "typescript_"), strings.HasPrefix(target.Rule, "js_"):
			return "typescript"
		case strings.HasPrefix(target.Rule, "solidity_"), strings.HasPrefix(target.Rule, "sol_"):
			return "solidity"
		}
	}
	return ""
}

// runExtractor runs the appropriate extractor for the language.
func (s *Syncer) runExtractor(language, pkgDir string) (*extraction.Result, error) {
	switch language {
	case "go":
		return s.runGoExtractor(pkgDir)
	case "rust":
		return s.runRustExtractor(pkgDir)
	case "python":
		return s.runPythonExtractor(pkgDir)
	case "typescript":
		return s.runTypescriptExtractor(pkgDir)
	case "solidity":
		return s.runSolidityExtractor(pkgDir)
	default:
		return nil, fmt.Errorf("unsupported language: %s", language)
	}
}

// runGoExtractor runs go-deps-extract on a directory.
func (s *Syncer) runGoExtractor(pkgDir string) (*extraction.Result, error) {
	extractorPath := s.config.GoExtractorPath
	if extractorPath == "" {
		extractorPath = "go-deps-extract"
	}

	// Check if extractor exists, fall back to go run
	_, err := exec.LookPath(extractorPath)
	if err != nil {
		// Fall back to using go list directly
		return s.extractGoImportsDirectly(pkgDir)
	}

	cmd := exec.Command(extractorPath, pkgDir)
	cmd.Dir = s.config.ProjectRoot

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("extractor failed: %s", string(exitErr.Stderr))
		}
		return nil, fmt.Errorf("running extractor: %w", err)
	}

	var result extraction.Result
	if err := json.Unmarshal(output, &result); err != nil {
		return nil, fmt.Errorf("parsing extractor output: %w", err)
	}

	return &result, nil
}

// runRustExtractor runs deps-extract for Rust on a directory.
func (s *Syncer) runRustExtractor(pkgDir string) (*extraction.Result, error) {
	return s.runDepsExtract("rust", pkgDir)
}

// runPythonExtractor runs deps-extract for Python on a directory.
func (s *Syncer) runPythonExtractor(pkgDir string) (*extraction.Result, error) {
	return s.runDepsExtract("python", pkgDir)
}

// runTypescriptExtractor runs deps-extract for TypeScript on a directory.
func (s *Syncer) runTypescriptExtractor(pkgDir string) (*extraction.Result, error) {
	return s.runDepsExtract("typescript", pkgDir)
}

// runSolidityExtractor runs deps-extract for Solidity on a directory.
func (s *Syncer) runSolidityExtractor(pkgDir string) (*extraction.Result, error) {
	return s.runDepsExtract("solidity", pkgDir)
}

// runDepsExtract runs the unified deps-extract tool for a given language.
func (s *Syncer) runDepsExtract(lang, pkgDir string) (*extraction.Result, error) {
	extractorPath := "deps-extract"

	// Check if extractor exists
	_, err := exec.LookPath(extractorPath)
	if err != nil {
		return nil, fmt.Errorf("deps-extract not found in PATH (install with: cargo install --path src/rust/deps-extract)")
	}

	cmd := exec.Command(extractorPath, "--lang", lang, pkgDir)
	cmd.Dir = s.config.ProjectRoot

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

// extractGoImportsDirectly uses go list directly when extractor isn't available.
func (s *Syncer) extractGoImportsDirectly(pkgDir string) (*extraction.Result, error) {
	result := extraction.NewResult("go")

	cmd := exec.Command("go", "list", "-json", "./...")
	cmd.Dir = pkgDir

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			result.AddError(fmt.Sprintf("go list warning: %s", string(exitErr.Stderr)))
		} else {
			return nil, fmt.Errorf("running go list: %w", err)
		}
	}

	// Get module path for internal classification
	modulePath := s.getModulePath()

	// Parse JSON stream
	dec := json.NewDecoder(strings.NewReader(string(output)))
	for dec.More() {
		var pkg struct {
			Dir         string
			ImportPath  string
			GoFiles     []string
			TestGoFiles []string
			Imports     []string
			TestImports []string
		}
		if err := dec.Decode(&pkg); err != nil {
			continue
		}

		// Calculate relative path
		relPath, err := filepath.Rel(s.config.ProjectRoot, pkg.Dir)
		if err != nil {
			relPath = pkg.Dir
		}

		// Classify imports
		var imports []extraction.Import
		for _, imp := range pkg.Imports {
			imports = append(imports, extraction.Import{
				Path: imp,
				Kind: classifyImport(imp, modulePath),
			})
		}

		var testImports []extraction.Import
		for _, imp := range pkg.TestImports {
			testImports = append(testImports, extraction.Import{
				Path: imp,
				Kind: classifyImport(imp, modulePath),
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

// getModulePath reads the module path from go.mod.
func (s *Syncer) getModulePath() string {
	modPath := filepath.Join(s.config.ProjectRoot, "go.mod")
	content, err := os.ReadFile(modPath)
	if err != nil {
		return ""
	}

	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "module ") {
			return strings.TrimSpace(strings.TrimPrefix(line, "module "))
		}
	}
	return ""
}

// classifyImport determines if an import is stdlib, external, or internal.
func classifyImport(imp, modulePath string) extraction.ImportKind {
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

// isLibraryTarget returns true if the rule is a library target.
func isLibraryTarget(rule string) bool {
	return strings.HasSuffix(rule, "_library") || rule == "go_library" || rule == "rust_library"
}

// isTestTarget returns true if the rule is a test target.
func isTestTarget(rule string) bool {
	return strings.HasSuffix(rule, "_test") || strings.Contains(rule, "test")
}

// hasLocalDep returns true if deps contains a local target dep (":foo").
func hasLocalDep(deps []string) bool {
	for _, d := range deps {
		if strings.HasPrefix(d, ":") {
			return true
		}
	}
	return false
}

// computeSelfTarget computes the Buck target for the current package.
// e.g., "/path/to/src/python/cargo" with projectRoot "/path/to" -> "//src/python/cargo:cargo"
func computeSelfTarget(pkgDir, projectRoot string) string {
	relPath, err := filepath.Rel(projectRoot, pkgDir)
	if err != nil {
		return ""
	}
	// relPath is like "src/python/cargo"
	targetName := filepath.Base(relPath)
	return fmt.Sprintf("//%s:%s", relPath, targetName)
}

// filterSelfReference removes deps that match the selfTarget.
func filterSelfReference(deps []mapper.MappedDep, selfTarget string) []mapper.MappedDep {
	if selfTarget == "" {
		return deps
	}
	var filtered []mapper.MappedDep
	for _, dep := range deps {
		if dep.Target != selfTarget {
			filtered = append(filtered, dep)
		}
	}
	return filtered
}

// mergePackageMappings combines mappings from multiple packages.
// This is needed for languages that have subdirectory structure (e.g., Solidity with src/ and test/).
func mergePackageMappings(mappings map[string]mapper.PackageMapping) mapper.PackageMapping {
	var result mapper.PackageMapping
	seenDeps := make(map[string]bool)
	seenTestDeps := make(map[string]bool)

	for _, m := range mappings {
		// Collect library deps (deduplicated)
		for _, dep := range m.Deps {
			if !seenDeps[dep.Target] {
				seenDeps[dep.Target] = true
				result.Deps = append(result.Deps, dep)
			}
		}
		// Collect test deps (deduplicated)
		for _, dep := range m.TestDeps {
			if !seenTestDeps[dep.Target] {
				seenTestDeps[dep.Target] = true
				result.TestDeps = append(result.TestDeps, dep)
			}
		}
		// Collect unmapped imports
		result.UnmappedImports = append(result.UnmappedImports, m.UnmappedImports...)
	}

	return result
}

// mergeWithPreserved merges new deps with preserved deps from old list.
// Preserves:
// - Local target deps (starting with ":") - these are manual same-package deps
// - TODO: deps between preserve markers
func mergeWithPreserved(oldDeps, newDeps []string) []string {
	// Build set of new deps for deduplication
	seen := make(map[string]bool)
	for _, d := range newDeps {
		seen[d] = true
	}

	// Preserve local target deps from old list (e.g., ":mylib")
	// These are manual dependencies on same-package targets
	var preserved []string
	for _, d := range oldDeps {
		if strings.HasPrefix(d, ":") && !seen[d] {
			preserved = append(preserved, d)
			seen[d] = true
		}
	}

	// Return preserved deps first, then new deps
	return append(preserved, newDeps...)
}

// diffDeps returns added and removed deps.
func diffDeps(oldDeps, newDeps []string) (added, removed []string) {
	oldSet := make(map[string]bool)
	for _, d := range oldDeps {
		oldSet[d] = true
	}

	newSet := make(map[string]bool)
	for _, d := range newDeps {
		newSet[d] = true
	}

	for _, d := range newDeps {
		if !oldSet[d] {
			added = append(added, d)
		}
	}

	for _, d := range oldDeps {
		if !newSet[d] {
			removed = append(removed, d)
		}
	}

	return added, removed
}

// stringSlicesEqual compares two string slices.
func stringSlicesEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
