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

	// Parse the rules.star file
	f, err := starlark.ParseFile(rulesPath)
	if err != nil {
		return nil, fmt.Errorf("parsing rules.star: %w", err)
	}

	// Determine package directory
	pkgDir := filepath.Dir(rulesPath)

	// Detect language from rules.star content
	language := s.detectLanguage(f)
	if language == "" {
		// Can't determine language, skip
		return result, nil
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

	// Find the package mapping (should be just one for single-directory extraction)
	var pkgMapping mapper.PackageMapping
	for _, m := range mappings {
		pkgMapping = m
		break
	}

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

			// Check if test has target_under_test - if so, library deps come transitively
			hasTargetUnderTest := target.GetStringAttr("target_under_test") != ""

			var newDeps []string
			seen := make(map[string]bool)

			if !hasTargetUnderTest {
				// No target_under_test, so include library deps
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

// runRustExtractor runs rust-deps-extract on a directory.
func (s *Syncer) runRustExtractor(pkgDir string) (*extraction.Result, error) {
	extractorPath := "rust-deps-extract"

	// Check if extractor exists, fall back to cargo metadata directly
	_, err := exec.LookPath(extractorPath)
	if err != nil {
		return s.extractRustDepsDirectly(pkgDir)
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

// extractRustDepsDirectly uses cargo metadata directly when extractor isn't available.
func (s *Syncer) extractRustDepsDirectly(pkgDir string) (*extraction.Result, error) {
	result := extraction.NewResult("rust")

	cmd := exec.Command("cargo", "metadata", "--format-version", "1", "--no-deps")
	cmd.Dir = pkgDir

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("cargo metadata failed: %s", string(exitErr.Stderr))
		}
		return nil, fmt.Errorf("running cargo metadata: %w", err)
	}

	var metadata struct {
		Packages []struct {
			Name         string `json:"name"`
			ID           string `json:"id"`
			ManifestPath string `json:"manifest_path"`
			Dependencies []struct {
				Name   string  `json:"name"`
				Source *string `json:"source"`
				Kind   *string `json:"kind"`
				Path   *string `json:"path"`
			} `json:"dependencies"`
			Targets []struct {
				Kind    []string `json:"kind"`
				SrcPath string   `json:"src_path"`
			} `json:"targets"`
		} `json:"packages"`
		WorkspaceMembers []string `json:"workspace_members"`
		WorkspaceRoot    string   `json:"workspace_root"`
	}

	if err := json.Unmarshal(output, &metadata); err != nil {
		return nil, fmt.Errorf("parsing cargo metadata: %w", err)
	}

	// Build workspace member set
	workspaceMembers := make(map[string]bool)
	for _, id := range metadata.WorkspaceMembers {
		workspaceMembers[id] = true
	}

	// Build workspace package name set
	workspacePackages := make(map[string]bool)
	for _, pkg := range metadata.Packages {
		if workspaceMembers[pkg.ID] {
			workspacePackages[pkg.Name] = true
		}
	}

	for _, pkg := range metadata.Packages {
		if !workspaceMembers[pkg.ID] {
			continue
		}

		// Calculate relative path
		pkgPath := filepath.Dir(pkg.ManifestPath)
		relPath, err := filepath.Rel(metadata.WorkspaceRoot, pkgPath)
		if err != nil {
			relPath = pkgPath
		}

		// Collect source files
		var files []string
		for _, target := range pkg.Targets {
			srcPath, err := filepath.Rel(metadata.WorkspaceRoot, target.SrcPath)
			if err != nil {
				srcPath = target.SrcPath
			}
			files = append(files, srcPath)
		}

		// Classify dependencies
		var imports []extraction.Import
		var testImports []extraction.Import

		for _, dep := range pkg.Dependencies {
			var kind extraction.ImportKind
			if dep.Source == nil {
				// Path dependency
				if workspacePackages[dep.Name] {
					kind = extraction.ImportKindInternal
				} else {
					kind = extraction.ImportKindInternal
				}
			} else {
				kind = extraction.ImportKindExternal
			}

			imp := extraction.Import{
				Path: dep.Name,
				Kind: kind,
			}

			if dep.Kind != nil && *dep.Kind == "dev" {
				testImports = append(testImports, imp)
			} else {
				imports = append(imports, imp)
			}
		}

		result.AddPackage(extraction.Package{
			Path:        relPath,
			Files:       files,
			Imports:     imports,
			TestImports: testImports,
		})
	}

	return result, nil
}

// runPythonExtractor runs python-deps-extract on a directory.
func (s *Syncer) runPythonExtractor(pkgDir string) (*extraction.Result, error) {
	extractorPath := "python-deps-extract"

	// Check if extractor exists
	_, err := exec.LookPath(extractorPath)
	if err != nil {
		// No built-in fallback for Python (need the extractor)
		return nil, fmt.Errorf("python-deps-extract not found in PATH")
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

// runTypescriptExtractor runs ts-deps-extract on a directory.
func (s *Syncer) runTypescriptExtractor(pkgDir string) (*extraction.Result, error) {
	extractorPath := "ts-deps-extract"

	// Check if extractor exists
	_, err := exec.LookPath(extractorPath)
	if err != nil {
		// No built-in fallback for TypeScript (need the extractor)
		return nil, fmt.Errorf("ts-deps-extract not found in PATH")
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

// mergeWithPreserved merges new deps with preserved deps from old list.
// TODO: Implement preserve marker support
func mergeWithPreserved(oldDeps, newDeps []string) []string {
	// For now, just return new deps
	// Future: parse preserve markers and keep those deps
	return newDeps
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
