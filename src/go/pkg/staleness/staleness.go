// Package staleness provides utilities for detecting when generated files
// are stale relative to their source files.
package staleness

import (
	"os"
	"path/filepath"
	"time"
)

// Result contains detailed information about a staleness check.
type Result struct {
	// Stale is true if the target is older than any source.
	Stale bool

	// TargetPath is the path to the target file.
	TargetPath string

	// TargetModTime is the modification time of the target, or zero if missing.
	TargetModTime time.Time

	// TargetMissing is true if the target file does not exist.
	TargetMissing bool

	// Sources contains info about each source file.
	Sources []SourceInfo

	// NewestSource is the source with the most recent modification time.
	NewestSource *SourceInfo
}

// SourceInfo contains information about a single source file.
type SourceInfo struct {
	// Path is the file path.
	Path string

	// ModTime is the modification time.
	ModTime time.Time

	// Missing is true if the file does not exist.
	Missing bool
}

// IsStale checks if target is stale relative to any of the sources.
// Returns true if:
//   - target does not exist, or
//   - any source is newer than target
//
// Source paths may include glob patterns (e.g., "*.go", "**/*.go").
// Returns an error if a source pattern matches no files (unless it's a literal path).
func IsStale(sources []string, target string) (bool, error) {
	result, err := Check(sources, target)
	if err != nil {
		return false, err
	}
	return result.Stale, nil
}

// Check performs a detailed staleness check.
// Source paths may include glob patterns.
func Check(sources []string, target string) (*Result, error) {
	result := &Result{
		TargetPath: target,
	}

	// Get target info
	targetInfo, err := os.Stat(target)
	if os.IsNotExist(err) {
		result.TargetMissing = true
		result.Stale = true
	} else if err != nil {
		return nil, err
	} else {
		result.TargetModTime = targetInfo.ModTime()
	}

	// Expand and check each source
	for _, pattern := range sources {
		expanded, err := expandGlob(pattern)
		if err != nil {
			return nil, err
		}

		for _, path := range expanded {
			info := SourceInfo{Path: path}

			fileInfo, err := os.Stat(path)
			if os.IsNotExist(err) {
				info.Missing = true
			} else if err != nil {
				return nil, err
			} else {
				info.ModTime = fileInfo.ModTime()

				// Track newest source
				if result.NewestSource == nil || info.ModTime.After(result.NewestSource.ModTime) {
					result.NewestSource = &info
				}

				// Check if this source is newer than target
				if !result.TargetMissing && info.ModTime.After(result.TargetModTime) {
					result.Stale = true
				}
			}

			result.Sources = append(result.Sources, info)
		}
	}

	return result, nil
}

// expandGlob expands a glob pattern to matching file paths.
// If the pattern contains no glob characters, returns it as-is.
func expandGlob(pattern string) ([]string, error) {
	// Check if pattern contains glob characters
	if !containsGlobChar(pattern) {
		return []string{pattern}, nil
	}

	// Handle ** patterns (double-star for recursive matching)
	if containsDoubleStar(pattern) {
		return expandDoubleStar(pattern)
	}

	// Standard glob
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	// If no matches, return the pattern as-is (will be marked missing)
	if len(matches) == 0 {
		return []string{pattern}, nil
	}

	return matches, nil
}

// containsGlobChar returns true if the pattern contains glob metacharacters.
func containsGlobChar(pattern string) bool {
	for _, c := range pattern {
		switch c {
		case '*', '?', '[':
			return true
		}
	}
	return false
}

// containsDoubleStar returns true if the pattern contains **.
func containsDoubleStar(pattern string) bool {
	for i := 0; i < len(pattern)-1; i++ {
		if pattern[i] == '*' && pattern[i+1] == '*' {
			return true
		}
	}
	return false
}

// expandDoubleStar expands patterns containing ** for recursive matching.
func expandDoubleStar(pattern string) ([]string, error) {
	// Find the base directory (everything before the first **)
	var base string
	var suffix string

	for i := 0; i < len(pattern)-1; i++ {
		if pattern[i] == '*' && pattern[i+1] == '*' {
			base = pattern[:i]
			suffix = pattern[i+2:] // Skip **
			break
		}
	}

	if base == "" {
		base = "."
	} else {
		// Remove trailing separator
		base = filepath.Clean(base)
	}

	// Clean up suffix - remove leading separator
	if len(suffix) > 0 && (suffix[0] == '/' || suffix[0] == filepath.Separator) {
		suffix = suffix[1:]
	}

	var matches []string

	err := filepath.Walk(base, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // Skip errors (permission denied, etc.)
		}

		if info.IsDir() {
			return nil
		}

		// If there's a suffix pattern, check if it matches
		if suffix != "" {
			matched, err := filepath.Match(suffix, filepath.Base(path))
			if err != nil {
				return nil
			}
			if !matched {
				return nil
			}
		}

		matches = append(matches, path)
		return nil
	})

	if err != nil {
		return nil, err
	}

	if len(matches) == 0 {
		return []string{pattern}, nil
	}

	return matches, nil
}
