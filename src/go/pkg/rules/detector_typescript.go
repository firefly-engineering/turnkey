package rules

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// TypeScriptImportDetector detects imports from TypeScript/JavaScript source files.
type TypeScriptImportDetector struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string
}

// NewTypeScriptImportDetector creates a new TypeScript import detector.
func NewTypeScriptImportDetector(projectRoot string) (*TypeScriptImportDetector, error) {
	d := &TypeScriptImportDetector{
		ProjectRoot: projectRoot,
	}

	return d, nil
}

// DetectImports detects all imports from TypeScript/JavaScript source files in a directory.
func (d *TypeScriptImportDetector) DetectImports(dir string) ([]Import, error) {
	var imports []Import

	// Find all TypeScript/JavaScript files
	extensions := []string{"*.ts", "*.tsx", "*.js", "*.jsx", "*.mts", "*.cts"}
	var files []string

	for _, ext := range extensions {
		matches, err := filepath.Glob(filepath.Join(dir, ext))
		if err != nil {
			continue
		}
		files = append(files, matches...)
	}

	for _, file := range files {
		// Skip test files and declaration files
		baseName := filepath.Base(file)
		if strings.Contains(baseName, ".test.") ||
			strings.Contains(baseName, ".spec.") ||
			strings.HasSuffix(baseName, ".d.ts") {
			continue
		}

		fileImports, err := d.detectFileImports(file)
		if err != nil {
			continue
		}

		imports = append(imports, fileImports...)
	}

	return deduplicateImports(imports), nil
}

// importPattern matches ES6 import statements.
// Examples:
//   - import * as _ from "lodash";
//   - import { map } from "lodash";
//   - import lodash from "lodash";
//   - import "lodash";
//   - import type { Foo } from "bar";
var tsImportPattern = regexp.MustCompile(`^\s*import\s+(?:type\s+)?(?:[^'"]+\s+from\s+)?['"]([^'"]+)['"]`)

// requirePattern matches CommonJS require statements.
// Examples:
//   - const _ = require("lodash");
//   - require("lodash");
var requirePattern = regexp.MustCompile(`require\s*\(\s*['"]([^'"]+)['"]\s*\)`)

// dynamicImportPattern matches dynamic imports.
// Examples:
//   - import("lodash")
//   - await import("lodash")
var dynamicImportPattern = regexp.MustCompile(`import\s*\(\s*['"]([^'"]+)['"]\s*\)`)

// detectFileImports detects imports from a single TypeScript/JavaScript file.
func (d *TypeScriptImportDetector) detectFileImports(path string) ([]Import, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var imports []Import
	scanner := bufio.NewScanner(file)
	lineNum := 0

	relPath, _ := filepath.Rel(d.ProjectRoot, path)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()

		// Skip comments (basic check)
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "/*") {
			continue
		}

		// Check for ES6 import statements
		if matches := tsImportPattern.FindStringSubmatch(line); len(matches) > 1 {
			moduleName := matches[1]
			if !d.isRelativeImport(moduleName) {
				imports = append(imports, Import{
					Path:       d.normalizeModuleName(moduleName),
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   d.isBuiltIn(moduleName),
				})
			}
		}

		// Check for require statements
		if matches := requirePattern.FindAllStringSubmatch(line, -1); len(matches) > 0 {
			for _, match := range matches {
				if len(match) > 1 {
					moduleName := match[1]
					if !d.isRelativeImport(moduleName) {
						imports = append(imports, Import{
							Path:       d.normalizeModuleName(moduleName),
							SourceFile: relPath,
							Line:       lineNum,
							IsStdLib:   d.isBuiltIn(moduleName),
						})
					}
				}
			}
		}

		// Check for dynamic imports
		if matches := dynamicImportPattern.FindAllStringSubmatch(line, -1); len(matches) > 0 {
			for _, match := range matches {
				if len(match) > 1 {
					moduleName := match[1]
					if !d.isRelativeImport(moduleName) {
						imports = append(imports, Import{
							Path:       d.normalizeModuleName(moduleName),
							SourceFile: relPath,
							Line:       lineNum,
							IsStdLib:   d.isBuiltIn(moduleName),
						})
					}
				}
			}
		}
	}

	return imports, scanner.Err()
}

// isRelativeImport checks if an import is relative (starts with ./ or ../).
func (d *TypeScriptImportDetector) isRelativeImport(moduleName string) bool {
	return strings.HasPrefix(moduleName, "./") ||
		strings.HasPrefix(moduleName, "../") ||
		moduleName == "." ||
		moduleName == ".."
}

// normalizeModuleName extracts the package name from a module path.
// For scoped packages (@org/pkg), returns the full scoped name.
// For submodule imports (lodash/map), returns just the package name.
func (d *TypeScriptImportDetector) normalizeModuleName(moduleName string) string {
	// Handle scoped packages (@org/pkg)
	if strings.HasPrefix(moduleName, "@") {
		parts := strings.SplitN(moduleName, "/", 3)
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
		return moduleName
	}

	// For regular packages, return just the first component
	parts := strings.SplitN(moduleName, "/", 2)
	return parts[0]
}

// isBuiltIn checks if a module is a Node.js built-in module.
func (d *TypeScriptImportDetector) isBuiltIn(moduleName string) bool {
	builtIns := map[string]bool{
		"assert":              true,
		"async_hooks":         true,
		"buffer":              true,
		"child_process":       true,
		"cluster":             true,
		"console":             true,
		"constants":           true,
		"crypto":              true,
		"dgram":               true,
		"diagnostics_channel": true,
		"dns":                 true,
		"domain":              true,
		"events":              true,
		"fs":                  true,
		"http":                true,
		"http2":               true,
		"https":               true,
		"inspector":           true,
		"module":              true,
		"net":                 true,
		"os":                  true,
		"path":                true,
		"perf_hooks":          true,
		"process":             true,
		"punycode":            true,
		"querystring":         true,
		"readline":            true,
		"repl":                true,
		"stream":              true,
		"string_decoder":      true,
		"sys":                 true,
		"timers":              true,
		"tls":                 true,
		"trace_events":        true,
		"tty":                 true,
		"url":                 true,
		"util":                true,
		"v8":                  true,
		"vm":                  true,
		"wasi":                true,
		"worker_threads":      true,
		"zlib":                true,
	}

	// Handle node: prefix
	moduleName = strings.TrimPrefix(moduleName, "node:")

	return builtIns[moduleName]
}

// IsInternalImport checks if a module is internal to the monorepo.
func (d *TypeScriptImportDetector) IsInternalImport(moduleName string) bool {
	// Check if it's a local package by looking for package.json
	packageDir := filepath.Join(d.ProjectRoot, "src", "ts", moduleName)
	if _, err := os.Stat(filepath.Join(packageDir, "package.json")); err == nil {
		return true
	}
	return false
}

// DetectTestImports detects imports from TypeScript/JavaScript test files.
func (d *TypeScriptImportDetector) DetectTestImports(dir string) ([]Import, error) {
	var imports []Import

	// Find test files
	patterns := []string{
		filepath.Join(dir, "*.test.ts"),
		filepath.Join(dir, "*.test.tsx"),
		filepath.Join(dir, "*.test.js"),
		filepath.Join(dir, "*.test.jsx"),
		filepath.Join(dir, "*.spec.ts"),
		filepath.Join(dir, "*.spec.tsx"),
		filepath.Join(dir, "*.spec.js"),
		filepath.Join(dir, "*.spec.jsx"),
		filepath.Join(dir, "__tests__", "*.ts"),
		filepath.Join(dir, "__tests__", "*.tsx"),
		filepath.Join(dir, "__tests__", "*.js"),
		filepath.Join(dir, "__tests__", "*.jsx"),
	}

	for _, pattern := range patterns {
		files, err := filepath.Glob(pattern)
		if err != nil {
			continue
		}

		for _, file := range files {
			fileImports, err := d.detectFileImports(file)
			if err != nil {
				continue
			}
			imports = append(imports, fileImports...)
		}
	}

	return deduplicateImports(imports), nil
}
