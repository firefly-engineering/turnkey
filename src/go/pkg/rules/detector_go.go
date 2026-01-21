package rules

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"strings"
)

// GoImportDetector detects imports from Go source files.
type GoImportDetector struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// ModulePath is the Go module path (from go.mod).
	ModulePath string
}

// NewGoImportDetector creates a new Go import detector.
func NewGoImportDetector(projectRoot string) (*GoImportDetector, error) {
	d := &GoImportDetector{
		ProjectRoot: projectRoot,
	}

	// Try to read module path from go.mod
	modPath := filepath.Join(projectRoot, "go.mod")
	if content, err := os.ReadFile(modPath); err == nil {
		d.ModulePath = extractModulePath(string(content))
	}

	return d, nil
}

// extractModulePath extracts the module path from go.mod content.
func extractModulePath(content string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "module ") {
			return strings.TrimSpace(strings.TrimPrefix(line, "module"))
		}
	}
	return ""
}

// DetectImports detects all imports from Go source files in a directory.
func (d *GoImportDetector) DetectImports(dir string) ([]Import, error) {
	var imports []Import

	// Find all Go files
	files, err := filepath.Glob(filepath.Join(dir, "*.go"))
	if err != nil {
		return nil, err
	}

	fset := token.NewFileSet()

	for _, file := range files {
		// Skip test files for now (they have different deps)
		if strings.HasSuffix(file, "_test.go") {
			continue
		}

		fileImports, err := d.detectFileImports(fset, file)
		if err != nil {
			// Log error but continue with other files
			continue
		}

		imports = append(imports, fileImports...)
	}

	// Deduplicate imports
	return deduplicateImports(imports), nil
}

// detectFileImports detects imports from a single Go file.
func (d *GoImportDetector) detectFileImports(fset *token.FileSet, path string) ([]Import, error) {
	// Parse the file
	f, err := parser.ParseFile(fset, path, nil, parser.ImportsOnly)
	if err != nil {
		return nil, err
	}

	var imports []Import

	for _, imp := range f.Imports {
		// Remove quotes from import path
		importPath := strings.Trim(imp.Path.Value, "\"")

		pos := fset.Position(imp.Path.Pos())
		relPath, _ := filepath.Rel(d.ProjectRoot, path)

		imports = append(imports, Import{
			Path:       importPath,
			SourceFile: relPath,
			Line:       pos.Line,
			IsStdLib:   d.isStdLib(importPath),
		})
	}

	return imports, nil
}

// isStdLib checks if an import path is from the Go standard library.
func (d *GoImportDetector) isStdLib(importPath string) bool {
	// Standard library packages don't have dots in their first path component
	firstComponent := importPath
	if idx := strings.Index(importPath, "/"); idx > 0 {
		firstComponent = importPath[:idx]
	}

	// If it contains a dot, it's not stdlib (e.g., "github.com", "golang.org")
	return !strings.Contains(firstComponent, ".")
}

// IsInternalImport checks if an import is internal to the monorepo.
func (d *GoImportDetector) IsInternalImport(importPath string) bool {
	if d.ModulePath == "" {
		return false
	}
	return strings.HasPrefix(importPath, d.ModulePath)
}

// GetInternalPath returns the internal path relative to the module root.
// For example, "github.com/org/repo/src/go/pkg/foo" -> "src/go/pkg/foo"
func (d *GoImportDetector) GetInternalPath(importPath string) string {
	if d.ModulePath == "" {
		return ""
	}
	return strings.TrimPrefix(importPath, d.ModulePath+"/")
}

// DetectTestImports detects imports from Go test files.
func (d *GoImportDetector) DetectTestImports(dir string) ([]Import, error) {
	var imports []Import

	// Find all test files
	files, err := filepath.Glob(filepath.Join(dir, "*_test.go"))
	if err != nil {
		return nil, err
	}

	fset := token.NewFileSet()

	for _, file := range files {
		fileImports, err := d.detectFileImports(fset, file)
		if err != nil {
			continue
		}

		imports = append(imports, fileImports...)
	}

	return deduplicateImports(imports), nil
}

// deduplicateImports removes duplicate imports (by path).
func deduplicateImports(imports []Import) []Import {
	seen := make(map[string]bool)
	var unique []Import

	for _, imp := range imports {
		if !seen[imp.Path] {
			seen[imp.Path] = true
			unique = append(unique, imp)
		}
	}

	return unique
}

// DetectFromAST is an alternative that works on an already parsed AST.
func (d *GoImportDetector) DetectFromAST(f *ast.File, fset *token.FileSet, filename string) []Import {
	var imports []Import

	for _, imp := range f.Imports {
		importPath := strings.Trim(imp.Path.Value, "\"")
		pos := fset.Position(imp.Path.Pos())

		imports = append(imports, Import{
			Path:       importPath,
			SourceFile: filename,
			Line:       pos.Line,
			IsStdLib:   d.isStdLib(importPath),
		})
	}

	return imports
}
