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

	// Rust-specific configuration.
	Rust RustSyncConfig

	// Python-specific configuration.
	Python PythonSyncConfig

	// TypeScript-specific configuration.
	TypeScript TypeScriptSyncConfig

	// Solidity-specific configuration.
	Solidity SoliditySyncConfig
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

// RustSyncConfig configures Rust-specific sync behavior.
type RustSyncConfig struct {
	Enabled        bool
	InternalPrefix string
	ExternalCell   string
}

// PythonSyncConfig configures Python-specific sync behavior.
type PythonSyncConfig struct {
	Enabled        bool
	InternalPrefix string
	ExternalCell   string
}

// TypeScriptSyncConfig configures TypeScript/JavaScript-specific sync behavior.
type TypeScriptSyncConfig struct {
	Enabled        bool
	InternalPrefix string
	ExternalCell   string
}

// SoliditySyncConfig configures Solidity-specific sync behavior.
type SoliditySyncConfig struct {
	Enabled        bool
	InternalPrefix string
	ExternalCell   string
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
	case "rust":
		newDeps, unmapped, err = s.detectRustDeps(dir)
	case "python":
		newDeps, unmapped, err = s.detectPythonDeps(dir)
	case "typescript":
		newDeps, unmapped, err = s.detectTypeScriptDeps(dir)
	case "solidity":
		newDeps, unmapped, err = s.detectSolidityDeps(dir)
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
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.tsx")); len(matches) > 0 {
		return "typescript"
	}

	// Check for Solidity files (including in src/ subdirectory)
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.sol")); len(matches) > 0 {
		return "solidity"
	}
	if matches, _ := filepath.Glob(filepath.Join(dir, "src", "*.sol")); len(matches) > 0 {
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

// detectRustDeps detects Rust dependencies from source files.
func (s *Syncer) detectRustDeps(dir string) ([]Dependency, []Import, error) {
	detector, err := NewRustImportDetector(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	imports, err := detector.DetectImports(dir)
	if err != nil {
		return nil, nil, err
	}

	mapper, err := NewRustMapper(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	if s.Config.Rust.InternalPrefix != "" {
		mapper.SetInternalPrefix(s.Config.Rust.InternalPrefix)
	}
	if s.Config.Rust.ExternalCell != "" {
		mapper.SetExternalCell(s.Config.Rust.ExternalCell)
	}

	deps, unmapped := mapper.MapImports(imports)
	return deps, unmapped, nil
}

// detectPythonDeps detects Python dependencies from source files.
func (s *Syncer) detectPythonDeps(dir string) ([]Dependency, []Import, error) {
	detector, err := NewPythonImportDetector(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	imports, err := detector.DetectImports(dir)
	if err != nil {
		return nil, nil, err
	}

	mapper, err := NewPythonMapper(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	if s.Config.Python.InternalPrefix != "" {
		mapper.SetInternalPrefix(s.Config.Python.InternalPrefix)
	}
	if s.Config.Python.ExternalCell != "" {
		mapper.SetExternalCell(s.Config.Python.ExternalCell)
	}

	deps, unmapped := mapper.MapImports(imports)
	return deps, unmapped, nil
}

// detectTypeScriptDeps detects TypeScript/JavaScript dependencies from source files.
func (s *Syncer) detectTypeScriptDeps(dir string) ([]Dependency, []Import, error) {
	detector, err := NewTypeScriptImportDetector(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	imports, err := detector.DetectImports(dir)
	if err != nil {
		return nil, nil, err
	}

	mapper, err := NewTypeScriptMapper(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	if s.Config.TypeScript.InternalPrefix != "" {
		mapper.SetInternalPrefix(s.Config.TypeScript.InternalPrefix)
	}
	if s.Config.TypeScript.ExternalCell != "" {
		mapper.SetExternalCell(s.Config.TypeScript.ExternalCell)
	}

	deps, unmapped := mapper.MapImports(imports)
	return deps, unmapped, nil
}

// detectSolidityDeps detects Solidity dependencies from source files.
func (s *Syncer) detectSolidityDeps(dir string) ([]Dependency, []Import, error) {
	detector, err := NewSolidityImportDetector(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	imports, err := detector.DetectImports(dir)
	if err != nil {
		return nil, nil, err
	}

	mapper, err := NewSolidityMapper(s.Config.ProjectRoot)
	if err != nil {
		return nil, nil, err
	}

	if s.Config.Solidity.InternalPrefix != "" {
		mapper.SetInternalPrefix(s.Config.Solidity.InternalPrefix)
	}
	if s.Config.Solidity.ExternalCell != "" {
		mapper.SetExternalCell(s.Config.Solidity.ExternalCell)
	}

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

// generateNewContent generates updated rules.star content by editing in place.
// This preserves all original attributes and only updates the deps sections.
func (s *Syncer) generateNewContent(rf *RulesFile, newDeps []string) (string, error) {
	content := rf.RawContent
	lines := strings.Split(content, "\n")

	// Compute new hash
	hash := ComputeDepsHash(newDeps)

	// Update or add header
	content = s.updateHeader(lines, hash)

	// Process targets in reverse order so line numbers don't shift
	// as we modify earlier parts of the file
	for i := len(rf.Targets) - 1; i >= 0; i-- {
		target := rf.Targets[i]
		// Re-parse to get current line numbers after header update
		content = s.updateTargetDepsInContent(content, target.Name, newDeps, target.PreservedDeps)
	}

	return content, nil
}

// updateHeader updates or adds the turnkey header with hash.
func (s *Syncer) updateHeader(lines []string, hash string) string {
	header := fmt.Sprintf("# Auto-managed by turnkey. Hash: %s\n# Manual sections marked with turnkey:preserve-start/end are not modified.", hash)

	// Check if there's already a turnkey header
	if len(lines) > 0 && strings.HasPrefix(lines[0], "# Auto-managed by turnkey") {
		// Replace existing header (first two lines)
		if len(lines) > 1 && strings.HasPrefix(lines[1], "# Manual sections") {
			lines = lines[2:]
		} else {
			lines = lines[1:]
		}
		return header + "\n" + strings.Join(lines, "\n")
	}

	// Check if first line is a comment (preserve it or add header before)
	if len(lines) > 0 && strings.HasPrefix(strings.TrimSpace(lines[0]), "#") {
		// There's an existing comment block - replace it with our header
		// Skip comment lines until we find a non-comment
		i := 0
		for i < len(lines) && strings.HasPrefix(strings.TrimSpace(lines[i]), "#") {
			i++
		}
		// Skip any blank lines after comments
		for i < len(lines) && strings.TrimSpace(lines[i]) == "" {
			i++
		}
		return header + "\n\n" + strings.Join(lines[i:], "\n")
	}

	// No existing header - add new one
	return header + "\n\n" + strings.Join(lines, "\n")
}

// updateTargetDepsInContent finds a target by name and updates its deps section.
func (s *Syncer) updateTargetDepsInContent(content, targetName string, newDeps, preservedDeps []string) string {
	lines := strings.Split(content, "\n")
	indent := "        " // Default 8 spaces for deps items

	// Find the target by looking for name = "targetName"
	targetStartLine := -1
	targetEndLine := -1
	bracketCount := 0

	for i, line := range lines {
		trimmed := strings.TrimSpace(line)

		// Look for target name
		if strings.Contains(trimmed, fmt.Sprintf(`name = "%s"`, targetName)) {
			// Found the target - now find its start (opening paren before this line)
			for j := i - 1; j >= 0; j-- {
				if strings.Contains(lines[j], "(") {
					targetStartLine = j
					break
				}
			}
		}

		// Track bracket depth to find target end
		if targetStartLine >= 0 && targetEndLine < 0 {
			for _, ch := range line {
				if ch == '(' {
					bracketCount++
				} else if ch == ')' {
					bracketCount--
					if bracketCount == 0 {
						targetEndLine = i
						break
					}
				}
			}
		}

		if targetEndLine >= 0 {
			break
		}
	}

	if targetStartLine < 0 || targetEndLine < 0 {
		// Target not found - return content unchanged
		return content
	}

	// Find the deps = [ line within this target
	depsStartLine := -1
	depsEndLine := -1
	depsIndent := ""
	depsBracketCount := 0

	for i := targetStartLine; i <= targetEndLine && i < len(lines); i++ {
		line := lines[i]
		trimmed := strings.TrimSpace(line)

		// Find "deps = [" (but not "npm_deps" or similar)
		if depsStartLine < 0 && strings.HasPrefix(trimmed, "deps") && strings.Contains(trimmed, "=") && strings.Contains(line, "[") {
			depsStartLine = i
			// Extract the indentation
			depsIndent = line[:len(line)-len(strings.TrimLeft(line, " \t"))]
			indent = depsIndent + "    " // Add 4 more spaces for items
			depsBracketCount = 1
			// Check if the opening bracket is on this line
			if !strings.Contains(trimmed, "],") {
				continue
			}
		}

		// Track bracket depth to find deps end
		if depsStartLine >= 0 && depsEndLine < 0 && i > depsStartLine {
			for _, ch := range line {
				if ch == '[' {
					depsBracketCount++
				} else if ch == ']' {
					depsBracketCount--
					if depsBracketCount == 0 {
						depsEndLine = i
						break
					}
				}
			}
		}

		if depsEndLine >= 0 {
			break
		}
	}

	if depsStartLine < 0 {
		// No deps section found - don't add one automatically
		// (the original file structure should be preserved)
		return content
	}

	// Generate new deps content
	var newDepsLines []string
	newDepsLines = append(newDepsLines, depsIndent+"deps = [")
	newDepsLines = append(newDepsLines, indent+MarkerAutoStart)
	for _, dep := range newDeps {
		newDepsLines = append(newDepsLines, fmt.Sprintf("%s\"%s\",", indent, dep))
	}
	newDepsLines = append(newDepsLines, indent+MarkerAutoEnd)

	// Add preserved deps if any
	if len(preservedDeps) > 0 {
		newDepsLines = append(newDepsLines, indent+MarkerPreserveStart)
		for _, dep := range preservedDeps {
			newDepsLines = append(newDepsLines, fmt.Sprintf("%s\"%s\",", indent, dep))
		}
		newDepsLines = append(newDepsLines, indent+MarkerPreserveEnd)
	}

	newDepsLines = append(newDepsLines, depsIndent+"],")

	// Replace the deps section
	var result []string
	result = append(result, lines[:depsStartLine]...)
	result = append(result, newDepsLines...)
	result = append(result, lines[depsEndLine+1:]...)

	return strings.Join(result, "\n")
}

// addDepsSection adds a deps section to a target that doesn't have one.
func (s *Syncer) addDepsSection(content string, target *Target, newDeps []string) string {
	if len(newDeps) == 0 {
		return content // Nothing to add
	}

	lines := strings.Split(content, "\n")
	indent := "    "      // 4 spaces for deps =
	itemIndent := "        " // 8 spaces for items

	// Find a good place to insert deps (after name = or srcs =)
	insertLine := target.StartLine // Default to start of target

	for i := target.StartLine - 1; i < target.EndLine && i < len(lines); i++ {
		line := lines[i]
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "name =") || strings.HasPrefix(trimmed, "srcs =") {
			insertLine = i + 1
			// Extract indentation from this line
			indent = line[:len(line)-len(strings.TrimLeft(line, " \t"))]
			itemIndent = indent + "    "
		}
	}

	// Generate deps section
	var depsLines []string
	depsLines = append(depsLines, indent+"deps = [")
	depsLines = append(depsLines, itemIndent+MarkerAutoStart)
	for _, dep := range newDeps {
		depsLines = append(depsLines, fmt.Sprintf("%s\"%s\",", itemIndent, dep))
	}
	depsLines = append(depsLines, itemIndent+MarkerAutoEnd)
	depsLines = append(depsLines, indent+"],")

	// Insert deps section
	result := strings.Join(lines[:insertLine], "\n")
	result += "\n" + strings.Join(depsLines, "\n")
	if insertLine < len(lines) {
		result += "\n" + strings.Join(lines[insertLine:], "\n")
	}

	return result
}
