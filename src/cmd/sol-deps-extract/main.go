// sol-deps-extract extracts Solidity module dependencies and outputs the extraction protocol JSON.
//
// Usage:
//
//	sol-deps-extract [flags] [dir]
//
// By default, it analyzes the current directory. If a directory is provided,
// it analyzes Solidity files in that directory.
//
// Flags:
//
//	-o string
//	    Output file path (default: stdout)
//	-exclude string
//	    Comma-separated list of directory patterns to exclude (e.g., "node_modules,lib,out")
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

func main() {
	var (
		output  = flag.String("o", "", "Output file path (default: stdout)")
		exclude = flag.String("exclude", "node_modules,lib,out,cache,artifacts,forge-cache", "Comma-separated list of directory patterns to exclude")
	)
	flag.Parse()

	dir := "."
	if flag.NArg() > 0 {
		dir = flag.Arg(0)
	}

	excludePatterns := strings.Split(*exclude, ",")
	for i := range excludePatterns {
		excludePatterns[i] = strings.TrimSpace(excludePatterns[i])
	}

	result, err := extract(dir, excludePatterns)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}

	// Output
	var w = os.Stdout
	if *output != "" {
		f, err := os.Create(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error creating output file: %v\n", err)
			os.Exit(1)
		}
		defer f.Close()
		w = f
	}

	if err := result.Write(w); err != nil {
		fmt.Fprintf(os.Stderr, "Error writing output: %v\n", err)
		os.Exit(1)
	}
}

// Regex patterns for Solidity imports
var (
	// Simple import: import "path/to/file.sol";
	simpleImportRe = regexp.MustCompile(`import\s+["']([^"']+)["']\s*;`)
	// Named import: import { Symbol, Symbol2 } from "path/to/file.sol";
	namedImportRe = regexp.MustCompile(`import\s+\{[^}]*\}\s+from\s+["']([^"']+)["']\s*;`)
	// Aliased import: import * as Alias from "path/to/file.sol";
	aliasedImportRe = regexp.MustCompile(`import\s+\*\s+as\s+\w+\s+from\s+["']([^"']+)["']\s*;`)
	// Direct aliased import: import "path/to/file.sol" as Alias;
	directAliasImportRe = regexp.MustCompile(`import\s+["']([^"']+)["']\s+as\s+\w+\s*;`)
)

// OpenZeppelin and other common library prefixes
var commonLibraryPrefixes = []string{
	"@openzeppelin/",
	"@chainlink/",
	"@uniswap/",
	"@aave/",
	"@compound/",
	"@gnosis/",
	"@safe-global/",
	"forge-std/",
	"ds-test/",
	"solmate/",
	"solady/",
}

// extract analyzes Solidity files and extracts import dependencies.
func extract(dir string, excludePatterns []string) (*extraction.Result, error) {
	result := extraction.NewResult("solidity")

	absDir, err := filepath.Abs(dir)
	if err != nil {
		return nil, fmt.Errorf("getting absolute path: %w", err)
	}

	// Find packages by looking at directories
	packages := make(map[string]*extraction.Package)

	err = filepath.Walk(absDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip excluded directories
		if info.IsDir() {
			name := info.Name()
			if shouldExclude(name, excludePatterns) {
				return filepath.SkipDir
			}
			return nil
		}

		// Only process .sol files
		if !strings.HasSuffix(path, ".sol") {
			return nil
		}

		// Get relative path
		relPath, err := filepath.Rel(absDir, path)
		if err != nil {
			relPath = path
		}

		// Determine package path
		pkgDir := filepath.Dir(relPath)
		if pkgDir == "." {
			pkgDir = ""
		}

		// Create or get package
		pkg, exists := packages[pkgDir]
		if !exists {
			pkg = &extraction.Package{
				Path: pkgDir,
			}
			packages[pkgDir] = pkg
		}

		// Add file to package
		pkg.Files = append(pkg.Files, filepath.Base(relPath))

		// Parse imports from file
		imports, testImports, err := parseImports(path, filepath.Base(relPath))
		if err != nil {
			result.AddError(fmt.Sprintf("parsing %s: %v", relPath, err))
			return nil
		}

		// Merge imports
		pkg.Imports = mergeImports(pkg.Imports, imports)
		pkg.TestImports = mergeImports(pkg.TestImports, testImports)

		return nil
	})

	if err != nil {
		return nil, fmt.Errorf("walking directory: %w", err)
	}

	// Convert map to slice and sort
	for _, pkg := range packages {
		// Sort imports
		sortImports(pkg.Imports)
		sortImports(pkg.TestImports)
		result.AddPackage(*pkg)
	}

	// Sort packages by path
	sort.Slice(result.Packages, func(i, j int) bool {
		return result.Packages[i].Path < result.Packages[j].Path
	})

	return result, nil
}

// parseImports extracts import statements from a Solidity file.
func parseImports(path, filename string) (imports, testImports []extraction.Import, err error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, nil, err
	}
	defer file.Close()

	// Check if it's a test file
	isTestFile := strings.Contains(filename, ".t.") ||
		strings.HasSuffix(filename, "Test.sol") ||
		strings.HasSuffix(filename, "_test.sol") ||
		strings.Contains(strings.ToLower(filename), "test")

	seen := make(map[string]bool)
	scanner := bufio.NewScanner(file)
	inMultilineComment := false

	for scanner.Scan() {
		line := scanner.Text()

		// Handle multi-line comments
		if inMultilineComment {
			if strings.Contains(line, "*/") {
				inMultilineComment = false
				// Continue processing the rest of the line after */
				idx := strings.Index(line, "*/")
				line = line[idx+2:]
			} else {
				continue
			}
		}

		// Skip single-line comments
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//") {
			continue
		}

		// Check for multi-line comment start
		if strings.Contains(line, "/*") {
			if strings.Contains(line, "*/") {
				// Comment starts and ends on same line, remove it
				// Simple approach: skip lines with block comments for now
				continue
			}
			inMultilineComment = true
			continue
		}

		// Find all imports in this line
		var importPaths []string

		for _, match := range simpleImportRe.FindAllStringSubmatch(line, -1) {
			importPaths = append(importPaths, match[1])
		}
		for _, match := range namedImportRe.FindAllStringSubmatch(line, -1) {
			importPaths = append(importPaths, match[1])
		}
		for _, match := range aliasedImportRe.FindAllStringSubmatch(line, -1) {
			importPaths = append(importPaths, match[1])
		}
		for _, match := range directAliasImportRe.FindAllStringSubmatch(line, -1) {
			importPaths = append(importPaths, match[1])
		}

		for _, importPath := range importPaths {
			if importPath == "" || seen[importPath] {
				continue
			}
			seen[importPath] = true

			imp := classifyImport(importPath)

			if isTestFile {
				testImports = append(testImports, imp)
			} else {
				imports = append(imports, imp)
			}
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, nil, err
	}

	return imports, testImports, nil
}

// classifyImport determines the kind of a Solidity import.
func classifyImport(importPath string) extraction.Import {
	// Relative imports are internal
	if strings.HasPrefix(importPath, "./") || strings.HasPrefix(importPath, "../") {
		return extraction.Import{
			Path: importPath,
			Kind: extraction.ImportKindInternal,
		}
	}

	// Check for common library prefixes (external)
	for _, prefix := range commonLibraryPrefixes {
		if strings.HasPrefix(importPath, prefix) {
			// Extract package name from path
			pkgName := getPackageName(importPath)
			return extraction.Import{
				Path: pkgName,
				Kind: extraction.ImportKindExternal,
			}
		}
	}

	// Absolute paths starting with a package name are external
	// e.g., "openzeppelin-contracts/..." or "@openzeppelin/..."
	if strings.HasPrefix(importPath, "@") || !strings.HasPrefix(importPath, "/") {
		pkgName := getPackageName(importPath)
		return extraction.Import{
			Path: pkgName,
			Kind: extraction.ImportKindExternal,
		}
	}

	// Default to internal for unrecognized patterns
	return extraction.Import{
		Path: importPath,
		Kind: extraction.ImportKindInternal,
	}
}

// getPackageName extracts the package name from an import path.
// For "@openzeppelin/contracts/token/ERC20.sol", returns "@openzeppelin/contracts".
// For "forge-std/Test.sol", returns "forge-std".
func getPackageName(importPath string) string {
	if strings.HasPrefix(importPath, "@") {
		// Scoped package: @org/pkg/path -> @org/pkg
		parts := strings.SplitN(importPath, "/", 3)
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
		return importPath
	}

	// Regular package: pkg/path -> pkg
	parts := strings.SplitN(importPath, "/", 2)
	return parts[0]
}

// shouldExclude checks if a directory should be excluded.
func shouldExclude(name string, patterns []string) bool {
	// Always skip hidden directories
	if strings.HasPrefix(name, ".") {
		return true
	}

	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		// Handle wildcard patterns
		if strings.HasPrefix(pattern, "*") {
			suffix := pattern[1:]
			if strings.HasSuffix(name, suffix) {
				return true
			}
		} else if name == pattern || strings.Contains(name, pattern) {
			return true
		}
	}
	return false
}

// mergeImports merges two import slices, avoiding duplicates.
func mergeImports(a, b []extraction.Import) []extraction.Import {
	seen := make(map[string]bool)
	result := make([]extraction.Import, 0, len(a)+len(b))

	for _, imp := range a {
		if !seen[imp.Path] {
			seen[imp.Path] = true
			result = append(result, imp)
		}
	}

	for _, imp := range b {
		if !seen[imp.Path] {
			seen[imp.Path] = true
			result = append(result, imp)
		}
	}

	return result
}

// sortImports sorts imports by path.
func sortImports(imports []extraction.Import) {
	sort.Slice(imports, func(i, j int) bool {
		return imports[i].Path < imports[j].Path
	})
}
