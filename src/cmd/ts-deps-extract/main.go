// ts-deps-extract extracts TypeScript/JavaScript module dependencies and outputs the extraction protocol JSON.
//
// Usage:
//
//	ts-deps-extract [flags] [dir]
//
// By default, it analyzes the current directory. If a directory is provided,
// it analyzes TypeScript/JavaScript files in that directory.
//
// Flags:
//
//	-o string
//	    Output file path (default: stdout)
//	-exclude string
//	    Comma-separated list of directory patterns to exclude (e.g., "node_modules,dist,build")
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

// Node.js built-in modules
var nodeBuiltins = map[string]bool{
	"assert": true, "async_hooks": true, "buffer": true, "child_process": true,
	"cluster": true, "console": true, "constants": true, "crypto": true,
	"dgram": true, "diagnostics_channel": true, "dns": true, "domain": true,
	"events": true, "fs": true, "http": true, "http2": true, "https": true,
	"inspector": true, "module": true, "net": true, "os": true, "path": true,
	"perf_hooks": true, "process": true, "punycode": true, "querystring": true,
	"readline": true, "repl": true, "stream": true, "string_decoder": true,
	"sys": true, "timers": true, "tls": true, "trace_events": true, "tty": true,
	"url": true, "util": true, "v8": true, "vm": true, "wasi": true,
	"worker_threads": true, "zlib": true,
	// Node.js prefixed versions
	"node:assert": true, "node:async_hooks": true, "node:buffer": true,
	"node:child_process": true, "node:cluster": true, "node:console": true,
	"node:constants": true, "node:crypto": true, "node:dgram": true,
	"node:diagnostics_channel": true, "node:dns": true, "node:domain": true,
	"node:events": true, "node:fs": true, "node:http": true, "node:http2": true,
	"node:https": true, "node:inspector": true, "node:module": true,
	"node:net": true, "node:os": true, "node:path": true, "node:perf_hooks": true,
	"node:process": true, "node:punycode": true, "node:querystring": true,
	"node:readline": true, "node:repl": true, "node:stream": true,
	"node:string_decoder": true, "node:sys": true, "node:timers": true,
	"node:tls": true, "node:trace_events": true, "node:tty": true, "node:url": true,
	"node:util": true, "node:v8": true, "node:vm": true, "node:wasi": true,
	"node:worker_threads": true, "node:zlib": true,
}

func main() {
	var (
		output  = flag.String("o", "", "Output file path (default: stdout)")
		exclude = flag.String("exclude", "node_modules,dist,build,.next,coverage", "Comma-separated list of directory patterns to exclude")
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

// Regex patterns for imports
var (
	// ES6 imports: import ... from 'module' or import ... from "module"
	importFromRe = regexp.MustCompile(`import\s+(?:.*?\s+from\s+)?['"]([^'"]+)['"]`)
	// Dynamic imports: import('module') or import("module")
	dynamicImportRe = regexp.MustCompile(`import\s*\(\s*['"]([^'"]+)['"]\s*\)`)
	// CommonJS requires: require('module') or require("module")
	requireRe = regexp.MustCompile(`require\s*\(\s*['"]([^'"]+)['"]\s*\)`)
	// Export from: export ... from 'module'
	exportFromRe = regexp.MustCompile(`export\s+.*?\s+from\s+['"]([^'"]+)['"]`)
)

// extract analyzes TypeScript/JavaScript files and extracts import dependencies.
func extract(dir string, excludePatterns []string) (*extraction.Result, error) {
	result := extraction.NewResult("typescript")

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

		// Only process .ts, .tsx, .js, .jsx, .mjs, .cjs files
		ext := filepath.Ext(path)
		if !isJSOrTSFile(ext) {
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

// isJSOrTSFile checks if the extension is for a JavaScript or TypeScript file.
func isJSOrTSFile(ext string) bool {
	switch ext {
	case ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs":
		return true
	default:
		return false
	}
}

// parseImports extracts import statements from a TypeScript/JavaScript file.
func parseImports(path, filename string) (imports, testImports []extraction.Import, err error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, nil, err
	}
	defer file.Close()

	// Check if it's a test file
	isTestFile := strings.Contains(filename, ".test.") ||
		strings.Contains(filename, ".spec.") ||
		strings.HasSuffix(strings.TrimSuffix(filename, filepath.Ext(filename)), "_test")

	seen := make(map[string]bool)
	scanner := bufio.NewScanner(file)

	for scanner.Scan() {
		line := scanner.Text()

		// Skip comments (simple check)
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "/*") {
			continue
		}

		// Find all imports in this line
		var modules []string

		for _, match := range importFromRe.FindAllStringSubmatch(line, -1) {
			modules = append(modules, match[1])
		}
		for _, match := range dynamicImportRe.FindAllStringSubmatch(line, -1) {
			modules = append(modules, match[1])
		}
		for _, match := range requireRe.FindAllStringSubmatch(line, -1) {
			modules = append(modules, match[1])
		}
		for _, match := range exportFromRe.FindAllStringSubmatch(line, -1) {
			modules = append(modules, match[1])
		}

		for _, moduleName := range modules {
			if moduleName == "" || seen[moduleName] {
				continue
			}
			seen[moduleName] = true

			imp := classifyImport(moduleName)

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

// classifyImport determines the kind of a TypeScript/JavaScript import.
func classifyImport(moduleName string) extraction.Import {
	// Relative imports are internal
	if strings.HasPrefix(moduleName, "./") || strings.HasPrefix(moduleName, "../") {
		return extraction.Import{
			Path: moduleName,
			Kind: extraction.ImportKindInternal,
		}
	}

	// Handle node: prefix
	if strings.HasPrefix(moduleName, "node:") {
		return extraction.Import{
			Path: moduleName,
			Kind: extraction.ImportKindStdlib,
		}
	}

	// Get the package name (handle scoped packages like @org/pkg)
	pkgName := getPackageName(moduleName)

	// Node.js builtins are stdlib (check both full path and package name)
	// This handles cases like "fs/promises" where the base module "fs" is a builtin
	if nodeBuiltins[moduleName] || nodeBuiltins[pkgName] {
		return extraction.Import{
			Path: pkgName,
			Kind: extraction.ImportKindStdlib,
		}
	}

	// Everything else is external
	return extraction.Import{
		Path: pkgName,
		Kind: extraction.ImportKindExternal,
	}
}

// getPackageName extracts the package name from an import path.
// For "@org/pkg/subpath", returns "@org/pkg".
// For "pkg/subpath", returns "pkg".
func getPackageName(importPath string) string {
	if strings.HasPrefix(importPath, "@") {
		// Scoped package
		parts := strings.SplitN(importPath, "/", 3)
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
		return importPath
	}

	// Regular package
	parts := strings.SplitN(importPath, "/", 2)
	return parts[0]
}

// shouldExclude checks if a directory should be excluded.
func shouldExclude(name string, patterns []string) bool {
	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		if name == pattern || strings.Contains(name, pattern) {
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
