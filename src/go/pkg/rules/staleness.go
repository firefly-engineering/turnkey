package rules

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// StalenessResult contains the result of a staleness check.
type StalenessResult struct {
	// Stale is true if the rules.star file needs updating.
	Stale bool

	// Reason explains why the file is stale (or why it's fresh).
	Reason string

	// Tier indicates which tier detected the staleness (1, 2, or 3).
	Tier int

	// RulesFile is the path to the rules.star file.
	RulesFile string

	// SourceFiles are the source files that were checked.
	SourceFiles []string

	// ChangedFiles are source files that triggered staleness (if any).
	ChangedFiles []string
}

// StalenessChecker performs multi-tier staleness detection.
type StalenessChecker struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// UseGit enables Tier 1 (git status) checking.
	UseGit bool

	// UseMtime enables Tier 2 (modification time) checking.
	UseMtime bool

	// UseHash enables Tier 3 (content hash) checking.
	UseHash bool
}

// NewStalenessChecker creates a new checker with all tiers enabled.
func NewStalenessChecker(projectRoot string) *StalenessChecker {
	return &StalenessChecker{
		ProjectRoot: projectRoot,
		UseGit:      true,
		UseMtime:    true,
		UseHash:     true,
	}
}

// Check performs a multi-tier staleness check.
// Returns as soon as staleness is detected (short-circuit).
func (c *StalenessChecker) Check(rulesFile string, sourceFiles []string) (*StalenessResult, error) {
	result := &StalenessResult{
		RulesFile:   rulesFile,
		SourceFiles: sourceFiles,
	}

	// Check if rules.star exists
	rulesInfo, err := os.Stat(rulesFile)
	if os.IsNotExist(err) {
		result.Stale = true
		result.Reason = "rules.star does not exist"
		result.Tier = 0
		return result, nil
	} else if err != nil {
		return nil, fmt.Errorf("failed to stat %s: %w", rulesFile, err)
	}

	// Tier 1: Git status
	if c.UseGit {
		stale, changed, err := c.checkGitStatus(rulesFile, sourceFiles)
		if err != nil {
			// Git check failed, continue to next tier
			// This is not fatal - git might not be available
		} else if stale {
			result.Stale = true
			result.Reason = fmt.Sprintf("uncommitted source changes: %v", changed)
			result.Tier = 1
			result.ChangedFiles = changed
			return result, nil
		}
	}

	// Tier 2: Modification time
	mtimeStale := false
	var mtimeChanged []string
	if c.UseMtime {
		var err error
		mtimeStale, mtimeChanged, err = c.checkMtime(rulesInfo.ModTime(), sourceFiles)
		if err != nil {
			return nil, fmt.Errorf("mtime check failed: %w", err)
		}
	}

	// Tier 3: Content hash (most accurate)
	// Run this if mtime indicates stale OR if hash checking is enabled
	if c.UseHash && (mtimeStale || !c.UseMtime) {
		stale, reason, err := c.checkHash(rulesFile, sourceFiles)
		if err != nil {
			// Hash check failed - fall back to mtime result
			if mtimeStale {
				result.Stale = true
				result.Reason = fmt.Sprintf("source files newer than rules.star: %v (hash check failed: %v)", mtimeChanged, err)
				result.Tier = 2
				result.ChangedFiles = mtimeChanged
				return result, nil
			}
		} else if stale {
			// Hash confirms staleness
			result.Stale = true
			result.Reason = reason
			result.Tier = 3
			return result, nil
		} else {
			// Hash says fresh - trust it over mtime (mtime was a false positive)
			result.Stale = false
			result.Reason = reason
			return result, nil
		}
	} else if mtimeStale {
		// Hash checking disabled, use mtime result
		result.Stale = true
		result.Reason = fmt.Sprintf("source files newer than rules.star: %v", mtimeChanged)
		result.Tier = 2
		result.ChangedFiles = mtimeChanged
		return result, nil
	}

	result.Stale = false
	result.Reason = "rules.star is up-to-date"
	return result, nil
}

// checkGitStatus uses git status to detect uncommitted source changes.
// Returns true if sources have uncommitted changes but rules.star doesn't.
func (c *StalenessChecker) checkGitStatus(rulesFile string, sourceFiles []string) (bool, []string, error) {
	// Get git status for all files
	cmd := exec.Command("git", "status", "--porcelain", "--")
	args := append([]string{rulesFile}, sourceFiles...)
	cmd.Args = append(cmd.Args, args...)
	cmd.Dir = c.ProjectRoot

	output, err := cmd.Output()
	if err != nil {
		return false, nil, fmt.Errorf("git status failed: %w", err)
	}

	if len(output) == 0 {
		// No uncommitted changes
		return false, nil, nil
	}

	// Parse git status output
	lines := strings.Split(strings.TrimSpace(string(output)), "\n")
	rulesChanged := false
	var changedSources []string

	for _, line := range lines {
		if len(line) < 3 {
			continue
		}
		// Git status format: "XY filename" where X=staged, Y=unstaged
		status := line[:2]
		file := strings.TrimSpace(line[3:])

		// Check if this is a modified/added file (not deleted)
		if status[0] == 'D' || status[1] == 'D' {
			continue
		}

		if file == rulesFile || strings.HasSuffix(file, filepath.Base(rulesFile)) {
			rulesChanged = true
		} else {
			changedSources = append(changedSources, file)
		}
	}

	// Stale if sources changed but rules.star didn't
	if len(changedSources) > 0 && !rulesChanged {
		return true, changedSources, nil
	}

	return false, nil, nil
}

// checkMtime compares modification times.
// Returns true if any source is newer than the rules.star mtime.
func (c *StalenessChecker) checkMtime(rulesModTime time.Time, sourceFiles []string) (bool, []string, error) {
	var changedFiles []string

	for _, src := range sourceFiles {
		// Expand globs if needed
		matches, err := filepath.Glob(filepath.Join(c.ProjectRoot, src))
		if err != nil {
			return false, nil, fmt.Errorf("invalid glob pattern %s: %w", src, err)
		}

		for _, match := range matches {
			info, err := os.Stat(match)
			if err != nil {
				if os.IsNotExist(err) {
					continue
				}
				return false, nil, fmt.Errorf("failed to stat %s: %w", match, err)
			}

			if info.ModTime().After(rulesModTime) {
				relPath, _ := filepath.Rel(c.ProjectRoot, match)
				changedFiles = append(changedFiles, relPath)
			}
		}
	}

	return len(changedFiles) > 0, changedFiles, nil
}

// checkHash compares the stored hash in rules.star with computed hash from sources.
// This is the most accurate but slowest tier.
func (c *StalenessChecker) checkHash(rulesFile string, sourceFiles []string) (bool, string, error) {
	// Parse the rules.star file to get stored hash
	parser := NewParser()
	rf, err := parser.ParseFile(rulesFile)
	if err != nil {
		return false, "", fmt.Errorf("failed to parse rules file: %w", err)
	}

	// Get the directory containing rules.star
	dir := filepath.Dir(rulesFile)

	// Only check Go files for now (supported language)
	if !isSupportedLanguageDir(dir) {
		return false, "unsupported language", nil
	}

	// Create detector and mapper
	detector, err := NewGoImportDetector(c.ProjectRoot)
	if err != nil {
		return false, "", fmt.Errorf("failed to create detector: %w", err)
	}

	imports, err := detector.DetectImports(dir)
	if err != nil {
		return false, "", fmt.Errorf("failed to detect imports: %w", err)
	}

	mapper, err := NewGoMapper(c.ProjectRoot)
	if err != nil {
		return false, "", fmt.Errorf("failed to create mapper: %w", err)
	}

	// Map imports to deps
	deps, _ := mapper.MapImports(imports)

	// Convert to targets and compute hash
	targets := DepsToTargets(deps)
	computedHash := ComputeDepsHash(targets)

	// Compare hashes
	if rf.Hash == "" {
		// No hash stored in file - check if there are any deps
		if len(targets) == 0 {
			// No deps detected, and no hash stored - consider fresh
			return false, "no deps detected", nil
		}
		// Deps detected but no hash stored - stale
		return true, "no hash stored, deps detected", nil
	}

	if rf.Hash != computedHash {
		return true, fmt.Sprintf("hash mismatch: stored=%s computed=%s", rf.Hash, computedHash), nil
	}

	return false, "hash matches", nil
}

// CheckDirectory checks all rules.star files in a directory.
func (c *StalenessChecker) CheckDirectory(dir string) ([]*StalenessResult, error) {
	var results []*StalenessResult

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
			// Find source files in the same directory
			sourceDir := filepath.Dir(path)

			// Only check directories with supported languages (Go for now)
			if !isSupportedLanguageDir(sourceDir) {
				return nil // Skip unsupported languages
			}

			sourceFiles, err := c.findSourceFiles(sourceDir)
			if err != nil {
				return fmt.Errorf("failed to find source files for %s: %w", path, err)
			}

			result, err := c.Check(path, sourceFiles)
			if err != nil {
				return fmt.Errorf("failed to check %s: %w", path, err)
			}

			results = append(results, result)
		}

		return nil
	})

	return results, err
}

// isSupportedLanguageDir returns true if the directory contains files from a supported language.
// Currently only Go is supported.
func isSupportedLanguageDir(dir string) bool {
	// Check for Go files
	if matches, _ := filepath.Glob(filepath.Join(dir, "*.go")); len(matches) > 0 {
		return true
	}
	return false
}

// findSourceFiles finds source files in a directory based on common patterns.
func (c *StalenessChecker) findSourceFiles(dir string) ([]string, error) {
	var sources []string

	// Common source file extensions by language
	patterns := []string{
		"*.go",   // Go
		"*.rs",   // Rust
		"*.py",   // Python
		"*.ts",   // TypeScript
		"*.tsx",  // TypeScript React
		"*.js",   // JavaScript
		"*.jsx",  // JavaScript React
		"*.sol",  // Solidity
	}

	for _, pattern := range patterns {
		matches, err := filepath.Glob(filepath.Join(dir, pattern))
		if err != nil {
			continue
		}

		for _, match := range matches {
			relPath, _ := filepath.Rel(c.ProjectRoot, match)
			sources = append(sources, relPath)
		}
	}

	return sources, nil
}
