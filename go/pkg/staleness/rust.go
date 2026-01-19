// Package staleness provides Rust source file staleness detection.
package staleness

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// CheckRustSrcList compares the Rust source files declared in a rules.star file
// against the actual .rs files in the directory.
func CheckRustSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "rust_library")
	if err != nil {
		return nil, err
	}

	// Also try rust_binary if no rust_library found
	if declaredSrcs == nil {
		declaredSrcs, err = parseBuckSrcs(buckFile, "rust_binary")
		if err != nil {
			return nil, err
		}
	}

	// Get actual .rs files
	actualSrcs, err := globRustSrcs(dir)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// CheckRustTestSrcList compares the Rust test files declared in a rules.star file
// against the actual test .rs files in the directory.
func CheckRustTestSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared test sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "rust_test")
	if err != nil {
		return nil, err
	}

	// Get actual test files (files in tests/ directory or *_test.rs)
	actualSrcs, err := globRustTestSrcs(dir)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// globRustSrcs finds Rust source files in a directory.
// Excludes test files and the tests/ subdirectory.
func globRustSrcs(dir string) ([]string, error) {
	pattern := filepath.Join(dir, "*.rs")
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	var result []string
	for _, path := range matches {
		base := filepath.Base(path)
		// Exclude test files
		if strings.HasSuffix(base, "_test.rs") {
			continue
		}
		result = append(result, base)
	}

	sort.Strings(result)
	return result, nil
}

// globRustTestSrcs finds Rust test files in a directory.
func globRustTestSrcs(dir string) ([]string, error) {
	var result []string

	// Check for *_test.rs files
	pattern := filepath.Join(dir, "*_test.rs")
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	for _, path := range matches {
		result = append(result, filepath.Base(path))
	}

	// Check for tests/*.rs files
	testsDir := filepath.Join(dir, "tests")
	if info, err := os.Stat(testsDir); err == nil && info.IsDir() {
		testPattern := filepath.Join(testsDir, "*.rs")
		testMatches, err := filepath.Glob(testPattern)
		if err != nil {
			return nil, err
		}
		for _, path := range testMatches {
			result = append(result, "tests/"+filepath.Base(path))
		}
	}

	sort.Strings(result)
	return result, nil
}

// RustImportResult contains the result of a Rust use statement comparison.
type RustImportResult struct {
	// Stale is true if the rules.star file's deps don't match actual use statements.
	Stale bool

	// BuckFile is the path to the rules.star file.
	BuckFile string

	// DeclaredDeps are the dependency targets declared in the rules.star file.
	DeclaredDeps []string

	// ActualUses are the external crate names found in Rust source files.
	ActualUses []string

	// Missing are crates used but not declared as deps.
	Missing []string

	// Extra are declared deps with no matching use.
	Extra []string
}

// CheckRustUses compares the Rust use statements in source files against
// the deps declared in the rules.star file.
func CheckRustUses(buckFile string) (*RustImportResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared deps from rules.star file
	declaredDeps, err := parseBuckDeps(buckFile, "rust_library")
	if err != nil {
		return nil, err
	}
	if declaredDeps == nil {
		declaredDeps, err = parseBuckDeps(buckFile, "rust_binary")
		if err != nil {
			return nil, err
		}
	}

	// Parse use statements from Rust source files
	uses, err := parseRustUses(dir)
	if err != nil {
		return nil, err
	}

	// Filter to external crates only
	externalUses := filterExternalRustUses(uses)

	return compareRustUsesAndDeps(buckFile, externalUses, declaredDeps), nil
}

// parseRustUses parses all Rust files in a directory and extracts external crate names.
func parseRustUses(dir string) ([]string, error) {
	pattern := filepath.Join(dir, "*.rs")
	files, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	useSet := make(map[string]bool)

	// Pattern to match use statements
	// use crate_name::...
	// use ::crate_name::...
	usePattern := regexp.MustCompile(`(?m)^use\s+(?:::)?([a-zA-Z_][a-zA-Z0-9_]*)`)

	// Pattern for extern crate
	externPattern := regexp.MustCompile(`(?m)^extern\s+crate\s+([a-zA-Z_][a-zA-Z0-9_]*)`)

	for _, file := range files {
		content, err := os.ReadFile(file)
		if err != nil {
			continue
		}

		// Extract use statements
		for _, match := range usePattern.FindAllSubmatch(content, -1) {
			if len(match) > 1 {
				useSet[string(match[1])] = true
			}
		}

		// Extract extern crate statements
		for _, match := range externPattern.FindAllSubmatch(content, -1) {
			if len(match) > 1 {
				useSet[string(match[1])] = true
			}
		}
	}

	// Convert to sorted slice
	var uses []string
	for use := range useSet {
		uses = append(uses, use)
	}
	sort.Strings(uses)

	return uses, nil
}

// filterExternalRustUses filters out standard library and crate-internal references.
func filterExternalRustUses(uses []string) []string {
	stdLib := map[string]bool{
		"std":        true,
		"core":       true,
		"alloc":      true,
		"proc_macro": true,
		"test":       true,
		"self":       true,
		"super":      true,
		"crate":      true,
	}

	var external []string
	for _, use := range uses {
		if !stdLib[use] {
			external = append(external, use)
		}
	}
	return external
}

// compareRustUsesAndDeps compares uses against declared deps.
func compareRustUsesAndDeps(buckFile string, uses, deps []string) *RustImportResult {
	result := &RustImportResult{
		BuckFile:     buckFile,
		ActualUses:   uses,
		DeclaredDeps: deps,
	}

	// Extract crate names from deps
	// Deps look like "rustdeps//vendor/crate_name:crate_name" or "//path:target"
	depCrates := make(map[string]bool)
	for _, dep := range deps {
		crate := extractRustDepCrate(dep)
		if crate != "" {
			depCrates[crate] = true
		}
	}

	// Find missing (uses without deps)
	for _, use := range uses {
		if !depCrates[use] {
			result.Missing = append(result.Missing, use)
		}
	}

	// Find extra deps (deps without matching uses)
	useSet := make(map[string]bool)
	for _, use := range uses {
		useSet[use] = true
	}
	for _, dep := range deps {
		crate := extractRustDepCrate(dep)
		if crate != "" && !useSet[crate] {
			result.Extra = append(result.Extra, dep)
		}
	}

	result.Stale = len(result.Missing) > 0 || len(result.Extra) > 0
	return result
}

// extractRustDepCrate extracts the crate name from a Buck target.
// For example:
//   "rustdeps//vendor/serde:serde" -> "serde"
//   "//crate/mylib:mylib" -> "" (local dep)
func extractRustDepCrate(dep string) string {
	// Look for rustdeps//vendor/ prefix
	if strings.Contains(dep, "rustdeps//vendor/") {
		// Extract crate name (last path component before :)
		idx := strings.LastIndex(dep, "/")
		if idx == -1 {
			return ""
		}
		crate := dep[idx+1:]
		if colonIdx := strings.Index(crate, ":"); colonIdx != -1 {
			crate = crate[:colonIdx]
		}
		return crate
	}
	return ""
}

// RustPackageResult contains the result of a Rust package staleness check.
type RustPackageResult struct {
	// BuckFile is the path to the rules.star file.
	BuckFile string

	// Stale is true if the rules.star file needs regeneration.
	Stale bool

	// FromCache is true if the result came from cache.
	FromCache bool

	// SrcFiles is the list of source files found.
	SrcFiles []string

	// Uses is the list of external crate uses found.
	Uses []string

	// SrcResult is the detailed source list result (nil if from cache).
	SrcResult *SrcListResult

	// UseResult is the detailed use statement result (nil if from cache).
	UseResult *RustImportResult
}

// CheckRustPackage performs a cached staleness check on a Rust package.
func (cc *CachedCheck) CheckRustPackage() (*RustPackageResult, error) {
	dir := filepath.Dir(cc.BuckFile)

	// Get current source files
	srcFiles, err := globRustSrcs(dir)
	if err != nil {
		return nil, err
	}

	// Get current uses
	uses, err := parseRustUses(dir)
	if err != nil {
		return nil, err
	}

	// Filter to external uses
	externalUses := filterExternalRustUses(uses)

	// Check if we can use cached result
	if !cc.Cache.NeedsCheck(cc.BuckFile, srcFiles, externalUses) {
		entry := cc.Cache.Get(cc.BuckFile)
		return &RustPackageResult{
			BuckFile:  cc.BuckFile,
			Stale:     entry.WasStale,
			FromCache: true,
			SrcFiles:  srcFiles,
			Uses:      externalUses,
		}, nil
	}

	// Perform full check
	srcResult, err := CheckRustSrcList(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	useResult, err := CheckRustUses(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	stale := srcResult.Stale || useResult.Stale

	// Update cache
	if err := cc.Cache.Update(cc.BuckFile, srcFiles, externalUses, stale); err != nil {
		// Log error but don't fail the check
	}

	return &RustPackageResult{
		BuckFile:  cc.BuckFile,
		Stale:     stale,
		FromCache: false,
		SrcFiles:  srcFiles,
		Uses:      externalUses,
		SrcResult: srcResult,
		UseResult: useResult,
	}, nil
}
