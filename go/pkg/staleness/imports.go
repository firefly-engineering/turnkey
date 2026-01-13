// Package staleness provides import parsing for dep staleness detection.
package staleness

import (
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// ImportResult contains the result of an import comparison.
type ImportResult struct {
	// Stale is true if the BUCK file's deps don't match actual imports.
	Stale bool

	// BuckFile is the path to the BUCK file.
	BuckFile string

	// DeclaredDeps are the dependency targets declared in the BUCK file.
	DeclaredDeps []string

	// ActualImports are the import paths found in Go source files.
	ActualImports []string

	// Missing are imports not covered by declared deps.
	Missing []string

	// Extra are declared deps with no matching import.
	Extra []string
}

// CheckGoImports compares the Go imports in source files against
// the deps declared in the BUCK file for the go_library rule.
//
// This parses Go source files using go/parser to extract import paths,
// then compares against deps in the BUCK file.
func CheckGoImports(buckFile string) (*ImportResult, error) {
	dir := filepath.Dir(buckFile)

	// Parse declared deps from BUCK file
	declaredDeps, err := parseBuckDeps(buckFile, "go_library")
	if err != nil {
		return nil, err
	}

	// Get package name from BUCK file (for self-reference filtering)
	pkgName, err := parseBuckPackageName(buckFile, "go_library")
	if err != nil {
		return nil, err
	}

	// Parse imports from Go source files
	imports, err := parseGoImports(dir, false)
	if err != nil {
		return nil, err
	}

	// Filter out standard library and self-references
	externalImports := filterExternalImports(imports, pkgName)

	return compareImportsAndDeps(buckFile, externalImports, declaredDeps), nil
}

// parseBuckDeps extracts the deps list from a specific rule type in a BUCK file.
func parseBuckDeps(buckFile, ruleType string) ([]string, error) {
	content, err := os.ReadFile(buckFile)
	if err != nil {
		return nil, err
	}

	text := string(content)

	// Find the rule block
	rulePattern := regexp.MustCompile(ruleType + `\s*\(`)
	ruleMatch := rulePattern.FindStringIndex(text)
	if ruleMatch == nil {
		return nil, nil
	}

	// Find the matching closing paren
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

	// Extract deps = [...] from the rule block
	depsPattern := regexp.MustCompile(`deps\s*=\s*\[((?:[^\[\]]|\n)*)\]`)
	depsMatch := depsPattern.FindStringSubmatch(ruleBlock)
	if depsMatch == nil {
		return nil, nil
	}

	// Parse the list of strings
	var deps []string
	stringPattern := regexp.MustCompile(`"([^"]+)"`)
	matches := stringPattern.FindAllStringSubmatch(depsMatch[1], -1)
	for _, m := range matches {
		deps = append(deps, m[1])
	}

	return deps, nil
}

// parseBuckPackageName extracts the package_name from a BUCK file.
func parseBuckPackageName(buckFile, ruleType string) (string, error) {
	content, err := os.ReadFile(buckFile)
	if err != nil {
		return "", err
	}

	text := string(content)

	// Find the rule block
	rulePattern := regexp.MustCompile(ruleType + `\s*\(`)
	ruleMatch := rulePattern.FindStringIndex(text)
	if ruleMatch == nil {
		return "", nil
	}

	// Find the matching closing paren
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

	// Extract package_name = "..."
	pkgPattern := regexp.MustCompile(`package_name\s*=\s*"([^"]+)"`)
	pkgMatch := pkgPattern.FindStringSubmatch(ruleBlock)
	if pkgMatch == nil {
		return "", nil
	}

	return pkgMatch[1], nil
}

// parseGoImports parses all Go files in a directory and extracts import paths.
// If testOnly is true, only parses test files; otherwise excludes test files.
func parseGoImports(dir string, testOnly bool) ([]string, error) {
	pattern := filepath.Join(dir, "*.go")
	files, err := filepath.Glob(pattern)
	if err != nil {
		return nil, err
	}

	importSet := make(map[string]bool)
	fset := token.NewFileSet()

	for _, file := range files {
		base := filepath.Base(file)
		isTest := strings.HasSuffix(base, "_test.go")
		if testOnly != isTest {
			continue
		}

		// Parse the file
		f, err := parser.ParseFile(fset, file, nil, parser.ImportsOnly)
		if err != nil {
			// Skip files that can't be parsed
			continue
		}

		// Extract imports
		for _, imp := range f.Imports {
			// Remove quotes from import path
			path := strings.Trim(imp.Path.Value, `"`)
			importSet[path] = true
		}
	}

	// Convert to sorted slice
	var imports []string
	for imp := range importSet {
		imports = append(imports, imp)
	}
	sort.Strings(imports)

	return imports, nil
}

// filterExternalImports filters out standard library and self-references.
func filterExternalImports(imports []string, selfPkg string) []string {
	var external []string
	for _, imp := range imports {
		// Skip standard library (no dots in path before first slash)
		if isStdLib(imp) {
			continue
		}
		// Skip self-references
		if selfPkg != "" && strings.HasPrefix(imp, selfPkg) {
			continue
		}
		external = append(external, imp)
	}
	return external
}

// isStdLib returns true if the import path appears to be a standard library package.
func isStdLib(path string) bool {
	// Standard library packages don't have dots before the first slash
	firstSlash := strings.Index(path, "/")
	var prefix string
	if firstSlash == -1 {
		prefix = path
	} else {
		prefix = path[:firstSlash]
	}
	return !strings.Contains(prefix, ".")
}

// compareImportsAndDeps compares imports against declared deps.
// Note: This is a simplified comparison. In practice, deps are Buck targets
// while imports are Go package paths, so we do a best-effort match.
func compareImportsAndDeps(buckFile string, imports, deps []string) *ImportResult {
	result := &ImportResult{
		BuckFile:      buckFile,
		ActualImports: imports,
		DeclaredDeps:  deps,
	}

	// Build a set of import prefixes that are covered by deps
	// Deps look like "//path:target" or "godeps//vendor/github.com/foo/bar:bar"
	// Imports look like "github.com/foo/bar"

	// Extract package paths from deps
	depPaths := make(map[string]bool)
	for _, dep := range deps {
		path := extractDepPath(dep)
		if path != "" {
			depPaths[path] = true
		}
	}

	// Check which imports have a matching dep
	importCovered := make(map[string]bool)
	for _, imp := range imports {
		if depCoversImport(depPaths, imp) {
			importCovered[imp] = true
		}
	}

	// Find missing (imports without deps)
	for _, imp := range imports {
		if !importCovered[imp] {
			result.Missing = append(result.Missing, imp)
		}
	}

	// Find extra deps (deps without matching imports)
	for _, dep := range deps {
		path := extractDepPath(dep)
		if path == "" {
			continue
		}
		found := false
		for _, imp := range imports {
			if strings.HasPrefix(imp, path) || strings.HasPrefix(path, imp) {
				found = true
				break
			}
		}
		if !found {
			result.Extra = append(result.Extra, dep)
		}
	}

	result.Stale = len(result.Missing) > 0 || len(result.Extra) > 0
	return result
}

// extractDepPath extracts the Go package path from a Buck target.
// For example:
//   "godeps//vendor/github.com/foo/bar:bar" -> "github.com/foo/bar"
//   "//go/pkg/mylib:mylib" -> "" (local dep, not external)
func extractDepPath(dep string) string {
	// Look for godeps//vendor/ prefix
	if strings.Contains(dep, "godeps//vendor/") {
		// Extract path after vendor/
		idx := strings.Index(dep, "vendor/")
		if idx == -1 {
			return ""
		}
		path := dep[idx+7:]
		// Remove :target suffix
		if colonIdx := strings.Index(path, ":"); colonIdx != -1 {
			path = path[:colonIdx]
		}
		return path
	}
	return ""
}

// depCoversImport checks if any dep path covers the given import.
func depCoversImport(depPaths map[string]bool, imp string) bool {
	// Exact match
	if depPaths[imp] {
		return true
	}
	// Check if any dep is a prefix of the import
	for path := range depPaths {
		if strings.HasPrefix(imp, path+"/") || imp == path {
			return true
		}
	}
	return false
}
