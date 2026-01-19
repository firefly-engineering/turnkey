// Package staleness provides Python source file staleness detection.
package staleness

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// CheckPythonSrcList compares the Python source files declared in a rules.star file
// against the actual .py files in the directory.
func CheckPythonSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "python_library")
	if err != nil {
		return nil, err
	}

	// Also try python_binary if no python_library found
	if declaredSrcs == nil {
		declaredSrcs, err = parseBuckSrcs(buckFile, "python_binary")
		if err != nil {
			return nil, err
		}
	}

	// Get actual .py files
	actualSrcs, err := globPythonSrcs(dir, false)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// CheckPythonTestSrcList compares the Python test files declared in a rules.star file
// against the actual test .py files in the directory.
func CheckPythonTestSrcList(buckFile string) (*SrcListResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared test sources from rules.star file
	declaredSrcs, err := parseBuckSrcs(buckFile, "python_test")
	if err != nil {
		return nil, err
	}

	// Get actual test files
	actualSrcs, err := globPythonSrcs(dir, true)
	if err != nil {
		return nil, err
	}

	return compareSrcLists(buckFile, declaredSrcs, actualSrcs), nil
}

// globPythonSrcs finds Python source files in a directory.
// If testOnly is true, returns only test_*.py and *_test.py files.
// If testOnly is false, returns only non-test .py files.
func globPythonSrcs(dir string, testOnly bool) ([]string, error) {
	pattern := filepath.Join(dir, "*.py")
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	var result []string
	for _, path := range matches {
		base := filepath.Base(path)
		isTest := strings.HasPrefix(base, "test_") || strings.HasSuffix(base, "_test.py")
		if testOnly == isTest {
			result = append(result, base)
		}
	}

	sort.Strings(result)
	return result, nil
}

// PythonImportResult contains the result of a Python import comparison.
type PythonImportResult struct {
	// Stale is true if the rules.star file's deps don't match actual imports.
	Stale bool

	// BuckFile is the path to the rules.star file.
	BuckFile string

	// DeclaredDeps are the dependency targets declared in the rules.star file.
	DeclaredDeps []string

	// ActualImports are the external package names found in Python source files.
	ActualImports []string

	// Missing are packages imported but not declared as deps.
	Missing []string

	// Extra are declared deps with no matching import.
	Extra []string
}

// CheckPythonImports compares the Python imports in source files against
// the deps declared in the rules.star file.
func CheckPythonImports(buckFile string) (*PythonImportResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared deps from rules.star file
	declaredDeps, err := parseBuckDeps(buckFile, "python_library")
	if err != nil {
		return nil, err
	}
	if declaredDeps == nil {
		declaredDeps, err = parseBuckDeps(buckFile, "python_binary")
		if err != nil {
			return nil, err
		}
	}

	// Parse import statements from Python source files
	imports, err := parsePythonImports(dir)
	if err != nil {
		return nil, err
	}

	// Filter to external packages only
	externalImports := filterExternalPythonImports(imports)

	return comparePythonImportsAndDeps(buckFile, externalImports, declaredDeps), nil
}

// parsePythonImports parses all Python files in a directory and extracts top-level package names.
func parsePythonImports(dir string) ([]string, error) {
	pattern := filepath.Join(dir, "*.py")
	files, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	importSet := make(map[string]bool)

	// Patterns to match import statements
	// import foo
	// import foo.bar
	// from foo import bar
	// from foo.bar import baz
	importPattern := regexp.MustCompile(`^import\s+([a-zA-Z_][a-zA-Z0-9_]*)`)
	fromPattern := regexp.MustCompile(`^from\s+([a-zA-Z_][a-zA-Z0-9_]*)`)

	for _, file := range files {
		f, err := os.Open(file)
		if err != nil {
			continue
		}

		scanner := bufio.NewScanner(f)
		for scanner.Scan() {
			line := strings.TrimSpace(scanner.Text())

			// Skip comments and empty lines
			if strings.HasPrefix(line, "#") || line == "" {
				continue
			}

			// Match import statements
			if match := importPattern.FindStringSubmatch(line); match != nil {
				importSet[match[1]] = true
			} else if match := fromPattern.FindStringSubmatch(line); match != nil {
				// Skip relative imports (from . import ...)
				if match[1] != "." {
					importSet[match[1]] = true
				}
			}
		}

		f.Close()
	}

	// Convert to sorted slice
	var imports []string
	for imp := range importSet {
		imports = append(imports, imp)
	}
	sort.Strings(imports)

	return imports, nil
}

// filterExternalPythonImports filters out standard library packages.
func filterExternalPythonImports(imports []string) []string {
	// Common Python standard library modules
	// This is not exhaustive but covers the most common ones
	stdLib := map[string]bool{
		"abc":           true,
		"argparse":      true,
		"asyncio":       true,
		"base64":        true,
		"collections":   true,
		"contextlib":    true,
		"copy":          true,
		"csv":           true,
		"dataclasses":   true,
		"datetime":      true,
		"decimal":       true,
		"enum":          true,
		"functools":     true,
		"glob":          true,
		"hashlib":       true,
		"hmac":          true,
		"html":          true,
		"http":          true,
		"importlib":     true,
		"io":            true,
		"itertools":     true,
		"json":          true,
		"logging":       true,
		"math":          true,
		"multiprocessing": true,
		"os":            true,
		"pathlib":       true,
		"pickle":        true,
		"platform":      true,
		"pprint":        true,
		"queue":         true,
		"random":        true,
		"re":            true,
		"shutil":        true,
		"signal":        true,
		"socket":        true,
		"sqlite3":       true,
		"ssl":           true,
		"string":        true,
		"struct":        true,
		"subprocess":    true,
		"sys":           true,
		"tempfile":      true,
		"textwrap":      true,
		"threading":     true,
		"time":          true,
		"traceback":     true,
		"typing":        true,
		"unittest":      true,
		"urllib":        true,
		"uuid":          true,
		"warnings":      true,
		"weakref":       true,
		"xml":           true,
		"zipfile":       true,
		"zlib":          true,
		// Test-related
		"pytest":        true,
		"__future__":    true,
	}

	var external []string
	for _, imp := range imports {
		if !stdLib[imp] {
			external = append(external, imp)
		}
	}
	return external
}

// comparePythonImportsAndDeps compares imports against declared deps.
func comparePythonImportsAndDeps(buckFile string, imports, deps []string) *PythonImportResult {
	result := &PythonImportResult{
		BuckFile:      buckFile,
		ActualImports: imports,
		DeclaredDeps:  deps,
	}

	// Extract package names from deps
	// Deps look like "pydeps//vendor/requests:requests" or "//path:target"
	depPkgs := make(map[string]bool)
	for _, dep := range deps {
		pkg := extractPythonDepPkg(dep)
		if pkg != "" {
			depPkgs[pkg] = true
		}
	}

	// Find missing (imports without deps)
	for _, imp := range imports {
		if !depPkgs[imp] {
			result.Missing = append(result.Missing, imp)
		}
	}

	// Find extra deps (deps without matching imports)
	importSet := make(map[string]bool)
	for _, imp := range imports {
		importSet[imp] = true
	}
	for _, dep := range deps {
		pkg := extractPythonDepPkg(dep)
		if pkg != "" && !importSet[pkg] {
			result.Extra = append(result.Extra, dep)
		}
	}

	result.Stale = len(result.Missing) > 0 || len(result.Extra) > 0
	return result
}

// extractPythonDepPkg extracts the package name from a Buck target.
// For example:
//   "pydeps//vendor/requests:requests" -> "requests"
//   "//lib/mylib:mylib" -> "" (local dep)
func extractPythonDepPkg(dep string) string {
	// Look for pydeps//vendor/ prefix
	if strings.Contains(dep, "pydeps//vendor/") {
		// Extract package name (last path component before :)
		idx := strings.LastIndex(dep, "/")
		if idx == -1 {
			return ""
		}
		pkg := dep[idx+1:]
		if colonIdx := strings.Index(pkg, ":"); colonIdx != -1 {
			pkg = pkg[:colonIdx]
		}
		return pkg
	}
	return ""
}

// PythonPackageResult contains the result of a Python package staleness check.
type PythonPackageResult struct {
	// BuckFile is the path to the rules.star file.
	BuckFile string

	// Stale is true if the rules.star file needs regeneration.
	Stale bool

	// FromCache is true if the result came from cache.
	FromCache bool

	// SrcFiles is the list of source files found.
	SrcFiles []string

	// Imports is the list of external imports found.
	Imports []string

	// SrcResult is the detailed source list result (nil if from cache).
	SrcResult *SrcListResult

	// ImportResult is the detailed import result (nil if from cache).
	ImportResult *PythonImportResult
}

// CheckPythonPackage performs a cached staleness check on a Python package.
func (cc *CachedCheck) CheckPythonPackage() (*PythonPackageResult, error) {
	dir := filepath.Dir(cc.BuckFile)

	// Get current source files
	srcFiles, err := globPythonSrcs(dir, false)
	if err != nil {
		return nil, err
	}

	// Get current imports
	imports, err := parsePythonImports(dir)
	if err != nil {
		return nil, err
	}

	// Filter to external imports
	externalImports := filterExternalPythonImports(imports)

	// Check if we can use cached result
	if !cc.Cache.NeedsCheck(cc.BuckFile, srcFiles, externalImports) {
		entry := cc.Cache.Get(cc.BuckFile)
		return &PythonPackageResult{
			BuckFile:  cc.BuckFile,
			Stale:     entry.WasStale,
			FromCache: true,
			SrcFiles:  srcFiles,
			Imports:   externalImports,
		}, nil
	}

	// Perform full check
	srcResult, err := CheckPythonSrcList(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	importResult, err := CheckPythonImports(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	stale := srcResult.Stale || importResult.Stale

	// Update cache
	if err := cc.Cache.Update(cc.BuckFile, srcFiles, externalImports, stale); err != nil {
		// Log error but don't fail the check
	}

	return &PythonPackageResult{
		BuckFile:     cc.BuckFile,
		Stale:        stale,
		FromCache:    false,
		SrcFiles:     srcFiles,
		Imports:      externalImports,
		SrcResult:    srcResult,
		ImportResult: importResult,
	}, nil
}
