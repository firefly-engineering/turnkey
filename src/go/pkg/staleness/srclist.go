// Package staleness provides source file list staleness detection.
package staleness

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// SrcListResult contains the result of a source file list comparison.
type SrcListResult struct {
	// Stale is true if the rules.star file's srcs don't match actual files.
	Stale bool

	// BuckFile is the path to the rules.star file.
	BuckFile string

	// DeclaredSrcs are the source files declared in the rules.star file.
	DeclaredSrcs []string

	// ActualSrcs are the source files found on disk.
	ActualSrcs []string

	// Missing are files declared in BUCK but not found on disk.
	Missing []string

	// Extra are files found on disk but not declared in BUCK.
	Extra []string
}

// CheckGoSrcList compares the Go source files declared in a rules.star file
// against the actual .go files in the directory. Test files (*_test.go)
// are compared separately from regular source files.
//
// This is a fast file-system based check that doesn't parse Go code.
func CheckGoSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "go_library")
	if err != nil {
		return nil, err
	}

	// Get actual .go files (excluding test files)
	actualSrcs, err := globGoSrcs(dir, false)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// CheckGoTestSrcList compares the Go test files declared in a rules.star file
// against the actual *_test.go files in the directory.
func CheckGoTestSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared test sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "go_test")
	if err != nil {
		return nil, err
	}

	// Get actual test files
	actualSrcs, err := globGoSrcs(dir, true)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// parseBuckSrcs extracts the srcs list from a specific rule type in a rules.star file.
// This is a simple regex-based parser that handles common patterns.
func parseBuckSrcs(buckFile, ruleType string) ([]string, error) {
	content, err := os.ReadFile(buckFile)
	if err != nil {
		return nil, err
	}

	// Pattern to match the rule and extract srcs
	// Handles both single-line and multi-line formats:
	//   srcs = ["file.go"],
	//   srcs = [
	//       "file1.go",
	//       "file2.go",
	//   ],
	text := string(content)

	// Find the rule block
	rulePattern := regexp.MustCompile(ruleType + `\s*\(`)
	ruleMatch := rulePattern.FindStringIndex(text)
	if ruleMatch == nil {
		// Rule not found, return empty list
		return nil, nil
	}

	// Find the matching closing paren by counting depth
	start := ruleMatch[1]
	depth := 1
	end := start
	for i := start; i < len(text) && depth > 0; i++ {
		switch text[i] {
		case '(':
			depth++
		case ')':
			depth--
		}
		end = i
	}

	ruleBlock := text[ruleMatch[0] : end+1]

	// Extract srcs = [...] from the rule block
	srcsPattern := regexp.MustCompile(`srcs\s*=\s*\[((?:[^\[\]]|\n)*)\]`)
	srcsMatch := srcsPattern.FindStringSubmatch(ruleBlock)
	if srcsMatch == nil {
		return nil, nil
	}

	// Parse the list of strings
	var srcs []string
	scanner := bufio.NewScanner(strings.NewReader(srcsMatch[1]))
	stringPattern := regexp.MustCompile(`"([^"]+)"`)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		matches := stringPattern.FindAllStringSubmatch(line, -1)
		for _, m := range matches {
			srcs = append(srcs, m[1])
		}
	}

	return srcs, nil
}

// globGoSrcs finds Go source files in a directory.
// If testOnly is true, returns only *_test.go files.
// If testOnly is false, returns only non-test .go files.
func globGoSrcs(dir string, testOnly bool) ([]string, error) {
	pattern := filepath.Join(dir, "*.go")
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	var result []string
	for _, path := range matches {
		base := filepath.Base(path)
		isTest := strings.HasSuffix(base, "_test.go")
		if testOnly == isTest {
			result = append(result, base)
		}
	}

	sort.Strings(result)
	return result, nil
}

// compareSrcLists compares declared and actual source lists.
func compareSrcLists(buckFile string, declared, actual []string) *SrcListResult {
	result := &SrcListResult{
		BuckFile:     buckFile,
		DeclaredSrcs: declared,
		ActualSrcs:   actual,
	}

	// Build sets for efficient lookup
	declaredSet := make(map[string]bool)
	for _, s := range declared {
		declaredSet[s] = true
	}

	actualSet := make(map[string]bool)
	for _, s := range actual {
		actualSet[s] = true
	}

	// Find missing (in BUCK but not on disk)
	for _, s := range declared {
		if !actualSet[s] {
			result.Missing = append(result.Missing, s)
		}
	}

	// Find extra (on disk but not in BUCK)
	for _, s := range actual {
		if !declaredSet[s] {
			result.Extra = append(result.Extra, s)
		}
	}

	result.Stale = len(result.Missing) > 0 || len(result.Extra) > 0
	return result
}
