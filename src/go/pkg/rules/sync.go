package rules

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// SyncConfig configures the rules sync behavior.
type SyncConfig struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// Enabled enables rules.star synchronization.
	Enabled bool

	// AutoSync enables automatic sync before builds.
	AutoSync bool

	// Strict fails if rules.star would change (for CI).
	Strict bool

	// DryRun shows what would change without writing files.
	DryRun bool

	// Go-specific configuration.
	Go GoSyncConfig
}

// GoSyncConfig configures Go-specific sync behavior.
type GoSyncConfig struct {
	// Enabled enables Go rules sync.
	Enabled bool

	// InternalPrefix is the Buck2 prefix for internal targets.
	InternalPrefix string

	// ExternalCell is the Buck2 cell for external deps.
	ExternalCell string
}

// SyncResult contains the result of a sync operation.
type SyncResult struct {
	// Path is the path to the rules.star file.
	Path string

	// Updated is true if the file was modified.
	Updated bool

	// Added are new dependencies that were added.
	Added []string

	// Removed are dependencies that were removed.
	Removed []string

	// Preserved are manually preserved dependencies.
	Preserved []string

	// Errors are any errors encountered.
	Errors []string
}

// Syncer synchronizes rules.star files based on source imports.
type Syncer struct {
	Config  SyncConfig
	Parser  *Parser
	Checker *StalenessChecker
}

// NewSyncer creates a new rules syncer.
func NewSyncer(config SyncConfig) *Syncer {
	return &Syncer{
		Config:  config,
		Parser:  NewParser(),
		Checker: NewStalenessChecker(config.ProjectRoot),
	}
}

// Check checks if any rules.star files are stale.
func (s *Syncer) Check() ([]*StalenessResult, error) {
	return s.Checker.CheckDirectory(s.Config.ProjectRoot)
}

// SyncDirectory synchronizes all rules.star files in a directory.
func (s *Syncer) SyncDirectory(dir string) ([]*SyncResult, error) {
	var results []*SyncResult

	// Find all rules.star files
	err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip directories that should be ignored
		if info.IsDir() && shouldSkipDirectory(info.Name()) {
			return filepath.SkipDir
		}

		if info.Name() == "rules.star" {
			// Only sync directories with supported languages (Go for now)
			sourceDir := filepath.Dir(path)
			if !isSupportedLanguageDir(sourceDir) {
				return nil // Skip unsupported languages
			}

			result, err := s.SyncFile(path)
			if err != nil {
				results = append(results, &SyncResult{
					Path:   path,
					Errors: []string{err.Error()},
				})
				return nil // Continue with other files
			}
			results = append(results, result)
		}

		return nil
	})

	return results, err
}

// shouldSkipDirectory returns true if the directory should be skipped during scanning.
func shouldSkipDirectory(name string) bool {
	// Skip hidden directories
	if strings.HasPrefix(name, ".") {
		return true
	}

	// Skip known build/output directories
	switch name {
	case "buck-out", "node_modules", "vendor", "__pycache__", "target":
		return true
	}

	return false
}

// SyncFile synchronizes a single rules.star file.
func (s *Syncer) SyncFile(rulesPath string) (*SyncResult, error) {
	result := &SyncResult{
		Path: rulesPath,
	}

	// Parse the existing rules.star
	rf, err := s.Parser.ParseFile(rulesPath)
	if err != nil {
		return nil, fmt.Errorf("failed to parse %s: %w", rulesPath, err)
	}

	// Get the directory containing rules.star
	dir := filepath.Dir(rulesPath)

	// Detect language based on file extensions
	lang := s.detectLanguage(dir)
	if lang == "" {
		result.Errors = append(result.Errors, "could not detect language")
		return result, nil
	}

	// Detect imports and map to dependencies based on language
	var newDeps []Dependency
	var unmapped []Import

	switch lang {
	case "go":
		newDeps, unmapped, err = s.detectGoDeps(dir)
	default:
		result.Errors = append(result.Errors, fmt.Sprintf("unsupported language: %s", lang))
		return result, nil
	}

	if err != nil {
		return nil, fmt.Errorf("failed to detect deps: %w", err)
	}

	// Report unmapped imports as warnings
	for _, imp := range unmapped {
		result.Errors = append(result.Errors, fmt.Sprintf("unmapped import: %s (in %s:%d)", imp.Path, imp.SourceFile, imp.Line))
	}

	// Convert to target strings and sort
	newDepTargets := DepsToTargets(newDeps)
	sort.Strings(newDepTargets)

	// Check if hash is missing (needs update to add hash header)
	computedHash := ComputeDepsHash(newDepTargets)
	hashMissing := rf.Hash == ""
	hashChanged := rf.Hash != "" && rf.Hash != computedHash

	// Update each target in the rules file
	for _, target := range rf.Targets {
		// Compute changes
		added, removed := diffDeps(target.AutoDeps, newDepTargets)

		if len(added) > 0 || len(removed) > 0 {
			result.Updated = true
			result.Added = append(result.Added, added...)
			result.Removed = append(result.Removed, removed...)
		}

		result.Preserved = append(result.Preserved, target.PreservedDeps...)
	}

	// Also mark as updated if hash needs to be added/updated
	if hashMissing || hashChanged {
		result.Updated = true
	}

	// If no changes needed, return early
	if !result.Updated {
		return result, nil
	}

	// Generate new content
	newContent, err := s.generateNewContent(rf, newDepTargets)
	if err != nil {
		return nil, fmt.Errorf("failed to generate content: %w", err)
	}

	// Check strict mode - fail if changes would be made (for CI)
	if s.Config.Strict {
		return result, fmt.Errorf("rules.star would change (strict mode): %s", rulesPath)
	}

	// Skip writing in dry-run mode
	if s.Config.DryRun {
		return result, nil
	}

	// Write the updated file
	if err := os.WriteFile(rulesPath, []byte(newContent), 0644); err != nil {
		return nil, fmt.Errorf("failed to write %s: %w", rulesPath, err)
	}

	return result, nil
}

// detectLanguage detects the primary language in a directory.
func (s *Syncer) detectLanguage(dir string) string {
	// Check for Go files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.go")); len(matches) > 0 {
		return "go"
	}

	// Check for Rust files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.rs")); len(matches) > 0 {
		return "rust"
	}

	// Check for Python files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.py")); len(matches) > 0 {
		return "python"
	}

	// Check for TypeScript files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.ts")); len(matches) > 0 {
		return "typescript"
	}

	// Check for Solidity files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.sol")); len(matches) > 0 {
		return "solidity"
	}

	return ""
}

// detectGoDeps detects Go dependencies from source files.
func (s *Syncer) detectGoDeps(dir string) ([]Dependency, []Import, error) {
	// Create detector
	detector, err := NewGoImportDetector(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	// Detect imports
	imports, err := detector.DetectImports(dir)
	if err != nil {
		return nil, nil, err
	}

	// Create mapper
	mapper, err := NewGoMapper(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	// Apply configuration
	if s.Config.Go.InternalPrefix != "" {
		mapper.SetInternalPrefix(s.Config.Go.InternalPrefix)
	}
	if s.Config.Go.ExternalCell != "" {
		mapper.SetExternalCell(s.Config.Go.ExternalCell)
	}

	// Map imports to dependencies
	deps, unmapped := mapper.MapImports(imports)

	return deps, unmapped, nil
}

// diffDeps computes the difference between old and new deps.
func diffDeps(old, new []string) (added, removed []string) {
	oldSet := make(map[string]bool)
	newSet := make(map[string]bool)

	for _, d := range old {
		oldSet[d] = true
	}
	for _, d := range new {
		newSet[d] = true
	}

	for _, d := range new {
		if !oldSet[d] {
			added = append(added, d)
		}
	}

	for _, d := range old {
		if !newSet[d] {
			removed = append(removed, d)
		}
	}

	return added, removed
}

// generateNewContent generates updated rules.star content.
func (s *Syncer) generateNewContent(rf *RulesFile, newDeps []string) (string, error) {
	var lines []string

	// Add header with hash
	hash := ComputeDepsHash(newDeps)
	lines = append(lines, s.Parser.GenerateHeader(hash))

	// Add load statements
	for _, load := range rf.Loads {
		lines = append(lines, load)
	}
	if len(rf.Loads) > 0 {
		lines = append(lines, "")
	}

	// Generate each target
	for i, target := range rf.Targets {
		if i > 0 {
			lines = append(lines, "")
		}
		lines = append(lines, s.Parser.GenerateTarget(target, newDeps))
	}

	return strings.Join(lines, "\n") + "\n", nil
}
