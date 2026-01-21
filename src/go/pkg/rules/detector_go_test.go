package rules

import (
	"os"
	"path/filepath"
	"testing"
)

func TestExtractModulePath(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		expected string
	}{
		{
			name:     "simple module",
			content:  "module github.com/foo/bar\n\ngo 1.21",
			expected: "github.com/foo/bar",
		},
		{
			name:     "module with version",
			content:  "module github.com/foo/bar/v2\n\ngo 1.21",
			expected: "github.com/foo/bar/v2",
		},
		{
			name:     "module with extra whitespace",
			content:  "module   github.com/foo/bar  \n\ngo 1.21",
			expected: "github.com/foo/bar",
		},
		{
			name:     "no module line",
			content:  "go 1.21\n\nrequire (\n\tgithub.com/foo/bar v1.0.0\n)",
			expected: "",
		},
		{
			name:     "empty content",
			content:  "",
			expected: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := extractModulePath(tt.content)
			if result != tt.expected {
				t.Errorf("extractModulePath() = %q, expected %q", result, tt.expected)
			}
		})
	}
}

func TestGoImportDetector_IsStdLib(t *testing.T) {
	detector := &GoImportDetector{}

	tests := []struct {
		importPath string
		expected   bool
	}{
		// Standard library imports
		{"fmt", true},
		{"os", true},
		{"net/http", true},
		{"encoding/json", true},
		{"crypto/sha256", true},
		{"path/filepath", true},
		{"go/ast", true},
		{"go/parser", true},
		{"internal/cpu", true},

		// External imports (have dots in first component)
		{"github.com/foo/bar", false},
		{"golang.org/x/tools", false},
		{"go.uber.org/zap", false},
		{"gopkg.in/yaml.v2", false},
		{"k8s.io/client-go", false},
	}

	for _, tt := range tests {
		t.Run(tt.importPath, func(t *testing.T) {
			result := detector.isStdLib(tt.importPath)
			if result != tt.expected {
				t.Errorf("isStdLib(%q) = %v, expected %v", tt.importPath, result, tt.expected)
			}
		})
	}
}

func TestGoImportDetector_IsInternalImport(t *testing.T) {
	detector := &GoImportDetector{
		ModulePath: "github.com/org/repo",
	}

	tests := []struct {
		importPath string
		expected   bool
	}{
		// Internal imports
		{"github.com/org/repo/src/go/pkg/foo", true},
		{"github.com/org/repo/cmd/myapp", true},
		{"github.com/org/repo", true}, // Edge case: exact match

		// External imports
		{"github.com/other/repo", false},
		{"github.com/org/otherrepo", false}, // Different repo
		{"golang.org/x/tools", false},
		{"fmt", false},
	}

	for _, tt := range tests {
		t.Run(tt.importPath, func(t *testing.T) {
			result := detector.IsInternalImport(tt.importPath)
			if result != tt.expected {
				t.Errorf("IsInternalImport(%q) = %v, expected %v", tt.importPath, result, tt.expected)
			}
		})
	}

	// Test with empty module path
	emptyDetector := &GoImportDetector{}
	if emptyDetector.IsInternalImport("github.com/org/repo/pkg") {
		t.Error("IsInternalImport should return false when ModulePath is empty")
	}
}

func TestGoImportDetector_GetInternalPath(t *testing.T) {
	detector := &GoImportDetector{
		ModulePath: "github.com/org/repo",
	}

	tests := []struct {
		importPath string
		expected   string
	}{
		{"github.com/org/repo/src/go/pkg/foo", "src/go/pkg/foo"},
		{"github.com/org/repo/cmd/myapp", "cmd/myapp"},
		{"github.com/org/repo/pkg", "pkg"},
	}

	for _, tt := range tests {
		t.Run(tt.importPath, func(t *testing.T) {
			result := detector.GetInternalPath(tt.importPath)
			if result != tt.expected {
				t.Errorf("GetInternalPath(%q) = %q, expected %q", tt.importPath, result, tt.expected)
			}
		})
	}

	// Test with empty module path
	emptyDetector := &GoImportDetector{}
	if emptyDetector.GetInternalPath("github.com/org/repo/pkg") != "" {
		t.Error("GetInternalPath should return empty string when ModulePath is empty")
	}
}

func TestGoImportDetector_DetectImports(t *testing.T) {
	// Create a temporary directory with test Go files
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create a test Go file
	testFile := filepath.Join(tmpDir, "main.go")
	content := `package main

import (
	"fmt"
	"os"

	"github.com/google/uuid"
	"github.com/foo/bar"
)

func main() {
	fmt.Println(uuid.New())
}
`
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	detector := &GoImportDetector{
		ProjectRoot: tmpDir,
	}

	imports, err := detector.DetectImports(tmpDir)
	if err != nil {
		t.Fatalf("DetectImports failed: %v", err)
	}

	// Should have 4 imports
	if len(imports) != 4 {
		t.Errorf("Expected 4 imports, got %d: %v", len(imports), imports)
	}

	// Verify import paths and stdlib detection
	importMap := make(map[string]Import)
	for _, imp := range imports {
		importMap[imp.Path] = imp
	}

	// Check stdlib imports
	if imp, ok := importMap["fmt"]; !ok {
		t.Error("Expected 'fmt' import")
	} else if !imp.IsStdLib {
		t.Error("'fmt' should be marked as stdlib")
	}

	if imp, ok := importMap["os"]; !ok {
		t.Error("Expected 'os' import")
	} else if !imp.IsStdLib {
		t.Error("'os' should be marked as stdlib")
	}

	// Check external imports
	if imp, ok := importMap["github.com/google/uuid"]; !ok {
		t.Error("Expected 'github.com/google/uuid' import")
	} else if imp.IsStdLib {
		t.Error("'github.com/google/uuid' should not be marked as stdlib")
	}

	if imp, ok := importMap["github.com/foo/bar"]; !ok {
		t.Error("Expected 'github.com/foo/bar' import")
	} else if imp.IsStdLib {
		t.Error("'github.com/foo/bar' should not be marked as stdlib")
	}
}

func TestGoImportDetector_DetectImports_SkipsTestFiles(t *testing.T) {
	// Create a temporary directory
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create main.go
	mainFile := filepath.Join(tmpDir, "main.go")
	mainContent := `package main

import "github.com/prod/dep"

func main() {}
`
	if err := os.WriteFile(mainFile, []byte(mainContent), 0644); err != nil {
		t.Fatalf("Failed to write main file: %v", err)
	}

	// Create main_test.go
	testFile := filepath.Join(tmpDir, "main_test.go")
	testContent := `package main

import (
	"testing"
	"github.com/test/dep"
)

func TestMain(t *testing.T) {}
`
	if err := os.WriteFile(testFile, []byte(testContent), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	detector := &GoImportDetector{
		ProjectRoot: tmpDir,
	}

	imports, err := detector.DetectImports(tmpDir)
	if err != nil {
		t.Fatalf("DetectImports failed: %v", err)
	}

	// Should only have 1 import from main.go (not from test file)
	if len(imports) != 1 {
		t.Errorf("Expected 1 import (skipping test files), got %d: %v", len(imports), imports)
	}

	if imports[0].Path != "github.com/prod/dep" {
		t.Errorf("Expected 'github.com/prod/dep', got %q", imports[0].Path)
	}
}

func TestGoImportDetector_DetectTestImports(t *testing.T) {
	// Create a temporary directory
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create main_test.go
	testFile := filepath.Join(tmpDir, "main_test.go")
	testContent := `package main

import (
	"testing"
	"github.com/test/dep"
)

func TestMain(t *testing.T) {}
`
	if err := os.WriteFile(testFile, []byte(testContent), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	detector := &GoImportDetector{
		ProjectRoot: tmpDir,
	}

	imports, err := detector.DetectTestImports(tmpDir)
	if err != nil {
		t.Fatalf("DetectTestImports failed: %v", err)
	}

	// Should have 2 imports
	if len(imports) != 2 {
		t.Errorf("Expected 2 imports, got %d: %v", len(imports), imports)
	}

	// Verify we got the right imports
	importMap := make(map[string]bool)
	for _, imp := range imports {
		importMap[imp.Path] = true
	}

	if !importMap["testing"] {
		t.Error("Expected 'testing' import")
	}
	if !importMap["github.com/test/dep"] {
		t.Error("Expected 'github.com/test/dep' import")
	}
}

func TestDeduplicateImports(t *testing.T) {
	imports := []Import{
		{Path: "fmt", SourceFile: "a.go", Line: 1},
		{Path: "github.com/foo/bar", SourceFile: "a.go", Line: 2},
		{Path: "fmt", SourceFile: "b.go", Line: 1}, // Duplicate
		{Path: "os", SourceFile: "a.go", Line: 3},
		{Path: "github.com/foo/bar", SourceFile: "b.go", Line: 2}, // Duplicate
	}

	result := deduplicateImports(imports)

	if len(result) != 3 {
		t.Errorf("Expected 3 unique imports, got %d: %v", len(result), result)
	}

	// Verify paths
	paths := make(map[string]bool)
	for _, imp := range result {
		paths[imp.Path] = true
	}

	if !paths["fmt"] || !paths["github.com/foo/bar"] || !paths["os"] {
		t.Errorf("Missing expected imports in result: %v", result)
	}
}

func TestNewGoImportDetector(t *testing.T) {
	// Create a temporary directory with go.mod
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create go.mod
	goMod := filepath.Join(tmpDir, "go.mod")
	if err := os.WriteFile(goMod, []byte("module github.com/test/repo\n\ngo 1.21"), 0644); err != nil {
		t.Fatalf("Failed to write go.mod: %v", err)
	}

	detector, err := NewGoImportDetector(tmpDir)
	if err != nil {
		t.Fatalf("NewGoImportDetector failed: %v", err)
	}

	if detector.ProjectRoot != tmpDir {
		t.Errorf("ProjectRoot = %q, expected %q", detector.ProjectRoot, tmpDir)
	}

	if detector.ModulePath != "github.com/test/repo" {
		t.Errorf("ModulePath = %q, expected 'github.com/test/repo'", detector.ModulePath)
	}
}

func TestNewGoImportDetector_NoGoMod(t *testing.T) {
	// Create a temporary directory without go.mod
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	detector, err := NewGoImportDetector(tmpDir)
	if err != nil {
		t.Fatalf("NewGoImportDetector failed: %v", err)
	}

	if detector.ModulePath != "" {
		t.Errorf("ModulePath should be empty when no go.mod, got %q", detector.ModulePath)
	}
}
