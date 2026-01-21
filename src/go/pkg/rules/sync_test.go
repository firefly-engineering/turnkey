package rules

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewSyncer(t *testing.T) {
	config := SyncConfig{
		ProjectRoot: "/project",
		Enabled:     true,
		AutoSync:    true,
		Strict:      false,
		DryRun:      false,
		Go: GoSyncConfig{
			Enabled:        true,
			InternalPrefix: "//src/go",
			ExternalCell:   "godeps",
		},
	}

	syncer := NewSyncer(config)

	if syncer.Config.ProjectRoot != "/project" {
		t.Errorf("ProjectRoot = %q, expected '/project'", syncer.Config.ProjectRoot)
	}
	if syncer.Parser == nil {
		t.Error("Parser should not be nil")
	}
	if syncer.Checker == nil {
		t.Error("Checker should not be nil")
	}
}

func TestSyncer_DetectLanguage(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	syncer := &Syncer{}

	tests := []struct {
		name     string
		files    []string
		expected string
	}{
		{
			name:     "go files",
			files:    []string{"main.go", "helper.go"},
			expected: "go",
		},
		{
			name:     "rust files",
			files:    []string{"lib.rs", "main.rs"},
			expected: "rust",
		},
		{
			name:     "python files",
			files:    []string{"app.py", "util.py"},
			expected: "python",
		},
		{
			name:     "typescript files",
			files:    []string{"index.ts", "types.ts"},
			expected: "typescript",
		},
		{
			name:     "solidity files",
			files:    []string{"Contract.sol"},
			expected: "solidity",
		},
		{
			name:     "no source files",
			files:    []string{"readme.md", "config.json"},
			expected: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create a subdir for this test
			testDir := filepath.Join(tmpDir, tt.name)
			if err := os.MkdirAll(testDir, 0755); err != nil {
				t.Fatalf("Failed to create test dir: %v", err)
			}

			// Create the test files
			for _, file := range tt.files {
				if err := os.WriteFile(filepath.Join(testDir, file), []byte("content"), 0644); err != nil {
					t.Fatalf("Failed to create file %s: %v", file, err)
				}
			}

			result := syncer.detectLanguage(testDir)
			if result != tt.expected {
				t.Errorf("detectLanguage() = %q, expected %q", result, tt.expected)
			}
		})
	}
}

func TestDiffDeps(t *testing.T) {
	tests := []struct {
		name            string
		old             []string
		new             []string
		expectedAdded   []string
		expectedRemoved []string
	}{
		{
			name:            "no changes",
			old:             []string{"//a:a", "//b:b"},
			new:             []string{"//a:a", "//b:b"},
			expectedAdded:   nil,
			expectedRemoved: nil,
		},
		{
			name:            "add only",
			old:             []string{"//a:a"},
			new:             []string{"//a:a", "//b:b"},
			expectedAdded:   []string{"//b:b"},
			expectedRemoved: nil,
		},
		{
			name:            "remove only",
			old:             []string{"//a:a", "//b:b"},
			new:             []string{"//a:a"},
			expectedAdded:   nil,
			expectedRemoved: []string{"//b:b"},
		},
		{
			name:            "add and remove",
			old:             []string{"//a:a", "//b:b"},
			new:             []string{"//a:a", "//c:c"},
			expectedAdded:   []string{"//c:c"},
			expectedRemoved: []string{"//b:b"},
		},
		{
			name:            "empty to new",
			old:             []string{},
			new:             []string{"//a:a", "//b:b"},
			expectedAdded:   []string{"//a:a", "//b:b"},
			expectedRemoved: nil,
		},
		{
			name:            "all removed",
			old:             []string{"//a:a", "//b:b"},
			new:             []string{},
			expectedAdded:   nil,
			expectedRemoved: []string{"//a:a", "//b:b"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			added, removed := diffDeps(tt.old, tt.new)

			if !stringSliceEqual(added, tt.expectedAdded) {
				t.Errorf("added = %v, expected %v", added, tt.expectedAdded)
			}
			if !stringSliceEqual(removed, tt.expectedRemoved) {
				t.Errorf("removed = %v, expected %v", removed, tt.expectedRemoved)
			}
		})
	}
}

func TestSyncResult_Fields(t *testing.T) {
	result := &SyncResult{
		Path:      "/path/to/rules.star",
		Updated:   true,
		Added:     []string{"//new:dep"},
		Removed:   []string{"//old:dep"},
		Preserved: []string{"//manual:dep"},
		Errors:    []string{"warning: something"},
	}

	if result.Path != "/path/to/rules.star" {
		t.Errorf("Path = %q, expected '/path/to/rules.star'", result.Path)
	}
	if !result.Updated {
		t.Error("Expected Updated=true")
	}
	if len(result.Added) != 1 || result.Added[0] != "//new:dep" {
		t.Errorf("Added = %v, expected [//new:dep]", result.Added)
	}
	if len(result.Removed) != 1 || result.Removed[0] != "//old:dep" {
		t.Errorf("Removed = %v, expected [//old:dep]", result.Removed)
	}
	if len(result.Preserved) != 1 || result.Preserved[0] != "//manual:dep" {
		t.Errorf("Preserved = %v, expected [//manual:dep]", result.Preserved)
	}
	if len(result.Errors) != 1 {
		t.Errorf("Errors length = %d, expected 1", len(result.Errors))
	}
}

func TestSyncConfig_Fields(t *testing.T) {
	config := SyncConfig{
		ProjectRoot: "/project",
		Enabled:     true,
		AutoSync:    true,
		Strict:      true,
		DryRun:      true,
		Go: GoSyncConfig{
			Enabled:        true,
			InternalPrefix: "//src/go",
			ExternalCell:   "godeps",
		},
	}

	if config.ProjectRoot != "/project" {
		t.Errorf("ProjectRoot = %q, expected '/project'", config.ProjectRoot)
	}
	if !config.Enabled {
		t.Error("Expected Enabled=true")
	}
	if !config.AutoSync {
		t.Error("Expected AutoSync=true")
	}
	if !config.Strict {
		t.Error("Expected Strict=true")
	}
	if !config.DryRun {
		t.Error("Expected DryRun=true")
	}
	if !config.Go.Enabled {
		t.Error("Expected Go.Enabled=true")
	}
	if config.Go.InternalPrefix != "//src/go" {
		t.Errorf("Go.InternalPrefix = %q, expected '//src/go'", config.Go.InternalPrefix)
	}
	if config.Go.ExternalCell != "godeps" {
		t.Errorf("Go.ExternalCell = %q, expected 'godeps'", config.Go.ExternalCell)
	}
}

func TestSyncer_GenerateNewContent(t *testing.T) {
	syncer := &Syncer{
		Parser: NewParser(),
	}

	rf := &RulesFile{
		Path:  "/project/pkg/rules.star",
		Loads: []string{`load("@prelude//:rules.bzl", "go_library")`},
		Targets: []*Target{
			{
				Name:          "mylib",
				Rule:          "go_library",
				Srcs:          []string{"*.go"},
				PreservedDeps: []string{"//custom:dep"},
			},
		},
	}

	newDeps := []string{
		"//src/go/pkg/foo:foo",
		"godeps//vendor/github.com/bar:bar",
	}

	content, err := syncer.generateNewContent(rf, newDeps)
	if err != nil {
		t.Fatalf("generateNewContent failed: %v", err)
	}

	// Verify content structure
	if !strings.Contains(content, "Auto-managed by turnkey") {
		t.Error("Content should contain header comment")
	}
	if !strings.Contains(content, "Hash:") {
		t.Error("Content should contain hash")
	}
	if !strings.Contains(content, `load("@prelude//:rules.bzl"`) {
		t.Error("Content should contain load statement")
	}
	if !strings.Contains(content, "go_library(") {
		t.Error("Content should contain rule")
	}
	if !strings.Contains(content, `name = "mylib"`) {
		t.Error("Content should contain target name")
	}
	if !strings.Contains(content, MarkerAutoStart) {
		t.Error("Content should contain auto-start marker")
	}
	if !strings.Contains(content, MarkerAutoEnd) {
		t.Error("Content should contain auto-end marker")
	}
	if !strings.Contains(content, MarkerPreserveStart) {
		t.Error("Content should contain preserve-start marker")
	}
	if !strings.Contains(content, MarkerPreserveEnd) {
		t.Error("Content should contain preserve-end marker")
	}
	if !strings.Contains(content, "//custom:dep") {
		t.Error("Content should contain preserved dep")
	}
}

func TestSyncer_SyncFile_DryRun(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create go.mod
	goModContent := "module github.com/test/repo\n\ngo 1.21"
	if err := os.WriteFile(filepath.Join(tmpDir, "go.mod"), []byte(goModContent), 0644); err != nil {
		t.Fatalf("Failed to write go.mod: %v", err)
	}

	// Create go-deps.toml
	depsContent := `schema_version = 1

[deps."github.com/google/uuid"]
version = "v1.6.0"
hash = "sha256-abc"
`
	if err := os.WriteFile(filepath.Join(tmpDir, "go-deps.toml"), []byte(depsContent), 0644); err != nil {
		t.Fatalf("Failed to write go-deps.toml: %v", err)
	}

	// Create subdirectory with Go files and rules.star
	pkgDir := filepath.Join(tmpDir, "src", "go", "pkg", "mylib")
	if err := os.MkdirAll(pkgDir, 0755); err != nil {
		t.Fatalf("Failed to create pkg dir: %v", err)
	}

	// Create source file
	srcContent := `package mylib

import "github.com/google/uuid"

func NewID() string { return uuid.New().String() }
`
	if err := os.WriteFile(filepath.Join(pkgDir, "mylib.go"), []byte(srcContent), 0644); err != nil {
		t.Fatalf("Failed to write source file: %v", err)
	}

	// Create rules.star with old deps
	rulesContent := `go_library(
    name = "mylib",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
`
	rulesPath := filepath.Join(pkgDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte(rulesContent), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}

	// Get original content
	originalContent, _ := os.ReadFile(rulesPath)

	config := SyncConfig{
		ProjectRoot: tmpDir,
		Enabled:     true,
		AutoSync:    true,
		Strict:      false,
		DryRun:      true, // DryRun mode
		Go: GoSyncConfig{
			Enabled:        true,
			InternalPrefix: "//src/go",
			ExternalCell:   "godeps",
		},
	}

	syncer := NewSyncer(config)
	result, err := syncer.SyncFile(rulesPath)
	if err != nil {
		t.Fatalf("SyncFile failed: %v", err)
	}

	// Should report updates
	if !result.Updated {
		t.Error("Expected Updated=true")
	}

	// But file should NOT be modified (dry run)
	newContent, _ := os.ReadFile(rulesPath)
	if string(newContent) != string(originalContent) {
		t.Error("File should not be modified in dry run mode")
	}
}

func TestSyncer_SyncFile_Strict(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create go.mod
	goModContent := "module github.com/test/repo\n\ngo 1.21"
	if err := os.WriteFile(filepath.Join(tmpDir, "go.mod"), []byte(goModContent), 0644); err != nil {
		t.Fatalf("Failed to write go.mod: %v", err)
	}

	// Create go-deps.toml
	depsContent := `schema_version = 1

[deps."github.com/google/uuid"]
version = "v1.6.0"
hash = "sha256-abc"
`
	if err := os.WriteFile(filepath.Join(tmpDir, "go-deps.toml"), []byte(depsContent), 0644); err != nil {
		t.Fatalf("Failed to write go-deps.toml: %v", err)
	}

	// Create subdirectory with Go files and rules.star
	pkgDir := filepath.Join(tmpDir, "src", "go", "pkg", "mylib")
	if err := os.MkdirAll(pkgDir, 0755); err != nil {
		t.Fatalf("Failed to create pkg dir: %v", err)
	}

	// Create source file with import
	srcContent := `package mylib

import "github.com/google/uuid"

func NewID() string { return uuid.New().String() }
`
	if err := os.WriteFile(filepath.Join(pkgDir, "mylib.go"), []byte(srcContent), 0644); err != nil {
		t.Fatalf("Failed to write source file: %v", err)
	}

	// Create rules.star with EMPTY deps (should fail in strict mode)
	rulesContent := `go_library(
    name = "mylib",
    srcs = glob(["*.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)
`
	rulesPath := filepath.Join(pkgDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte(rulesContent), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}

	config := SyncConfig{
		ProjectRoot: tmpDir,
		Enabled:     true,
		AutoSync:    true,
		Strict:      true, // Strict mode
		DryRun:      false,
		Go: GoSyncConfig{
			Enabled:        true,
			InternalPrefix: "//src/go",
			ExternalCell:   "godeps",
		},
	}

	syncer := NewSyncer(config)
	_, err = syncer.SyncFile(rulesPath)

	// Should fail in strict mode when changes are needed
	if err == nil {
		t.Error("Expected error in strict mode when changes needed")
	}
	if !strings.Contains(err.Error(), "strict mode") {
		t.Errorf("Error should mention strict mode: %v", err)
	}
}

// Helper function to compare string slices
func stringSliceEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
