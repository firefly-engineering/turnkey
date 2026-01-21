package rules

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestNewStalenessChecker(t *testing.T) {
	checker := NewStalenessChecker("/project")

	if checker.ProjectRoot != "/project" {
		t.Errorf("ProjectRoot = %q, expected '/project'", checker.ProjectRoot)
	}
	if !checker.UseGit {
		t.Error("UseGit should be true by default")
	}
	if !checker.UseMtime {
		t.Error("UseMtime should be true by default")
	}
	if !checker.UseHash {
		t.Error("UseHash should be true by default")
	}
}

func TestStalenessChecker_Check_RulesFileNotExists(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	checker := NewStalenessChecker(tmpDir)

	rulesPath := filepath.Join(tmpDir, "rules.star")
	result, err := checker.Check(rulesPath, []string{})
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}

	if !result.Stale {
		t.Error("Expected stale=true when rules.star doesn't exist")
	}
	if result.Tier != 0 {
		t.Errorf("Tier = %d, expected 0 (file doesn't exist)", result.Tier)
	}
	if result.RulesFile != rulesPath {
		t.Errorf("RulesFile = %q, expected %q", result.RulesFile, rulesPath)
	}
}

func TestStalenessChecker_Check_MtimeStale(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create rules.star with old timestamp
	rulesPath := filepath.Join(tmpDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte("go_binary(name = \"test\")"), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}
	// Set old modification time
	oldTime := time.Now().Add(-24 * time.Hour)
	if err := os.Chtimes(rulesPath, oldTime, oldTime); err != nil {
		t.Fatalf("Failed to set mtime: %v", err)
	}

	// Create source file with newer timestamp
	srcPath := filepath.Join(tmpDir, "main.go")
	if err := os.WriteFile(srcPath, []byte("package main"), 0644); err != nil {
		t.Fatalf("Failed to write source file: %v", err)
	}

	// Disable git check (we don't have a git repo)
	checker := &StalenessChecker{
		ProjectRoot: tmpDir,
		UseGit:      false,
		UseMtime:    true,
		UseHash:     false,
	}

	relSrcPath, _ := filepath.Rel(tmpDir, srcPath)
	result, err := checker.Check(rulesPath, []string{relSrcPath})
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}

	if !result.Stale {
		t.Error("Expected stale=true when source is newer")
	}
	if result.Tier != 2 {
		t.Errorf("Tier = %d, expected 2 (mtime)", result.Tier)
	}
	if len(result.ChangedFiles) == 0 {
		t.Error("Expected ChangedFiles to be populated")
	}
}

func TestStalenessChecker_Check_Fresh(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create source file first
	srcPath := filepath.Join(tmpDir, "main.go")
	if err := os.WriteFile(srcPath, []byte("package main"), 0644); err != nil {
		t.Fatalf("Failed to write source file: %v", err)
	}

	// Wait a bit to ensure different timestamps
	time.Sleep(10 * time.Millisecond)

	// Create rules.star after (newer)
	rulesPath := filepath.Join(tmpDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte("go_binary(name = \"test\")"), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}

	// Disable git check (we don't have a git repo)
	checker := &StalenessChecker{
		ProjectRoot: tmpDir,
		UseGit:      false,
		UseMtime:    true,
		UseHash:     false,
	}

	relSrcPath, _ := filepath.Rel(tmpDir, srcPath)
	result, err := checker.Check(rulesPath, []string{relSrcPath})
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}

	if result.Stale {
		t.Error("Expected stale=false when rules.star is newer than sources")
	}
}

func TestStalenessChecker_CheckMtime_GlobPattern(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create rules.star with old timestamp
	rulesPath := filepath.Join(tmpDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte("go_binary(name = \"test\")"), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}
	rulesInfo, _ := os.Stat(rulesPath)
	oldTime := rulesInfo.ModTime().Add(-24 * time.Hour)
	os.Chtimes(rulesPath, oldTime, oldTime)

	// Create newer Go files
	for _, name := range []string{"main.go", "helper.go"} {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte("package main"), 0644); err != nil {
			t.Fatalf("Failed to write %s: %v", name, err)
		}
	}

	checker := &StalenessChecker{
		ProjectRoot: tmpDir,
		UseMtime:    true,
	}

	rulesInfo, _ = os.Stat(rulesPath)
	changed, changedFiles, err := checker.checkMtime(rulesInfo.ModTime(), []string{"*.go"})
	if err != nil {
		t.Fatalf("checkMtime failed: %v", err)
	}

	if !changed {
		t.Error("Expected changed=true with glob pattern")
	}
	if len(changedFiles) != 2 {
		t.Errorf("Expected 2 changed files, got %d", len(changedFiles))
	}
}

func TestStalenessChecker_FindSourceFiles(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create source files of different types
	files := map[string]string{
		"main.go":    "package main",
		"helper.go":  "package main",
		"lib.rs":     "fn main() {}",
		"app.py":     "print('hi')",
		"index.ts":   "console.log('hi')",
		"readme.md":  "# readme",  // Not a source file
		"rules.star": "go_binary", // Not a source file
	}

	for name, content := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte(content), 0644); err != nil {
			t.Fatalf("Failed to write %s: %v", name, err)
		}
	}

	checker := NewStalenessChecker(tmpDir)
	sources, err := checker.findSourceFiles(tmpDir)
	if err != nil {
		t.Fatalf("findSourceFiles failed: %v", err)
	}

	// Should find: main.go, helper.go, lib.rs, app.py, index.ts
	if len(sources) != 5 {
		t.Errorf("Expected 5 source files, got %d: %v", len(sources), sources)
	}

	// Verify that non-source files are excluded
	for _, src := range sources {
		if filepath.Base(src) == "readme.md" || filepath.Base(src) == "rules.star" {
			t.Errorf("Unexpected file in sources: %s", src)
		}
	}
}

func TestStalenessChecker_CheckDirectory(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create a subdirectory with rules.star
	subDir := filepath.Join(tmpDir, "pkg", "foo")
	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatalf("Failed to create subdir: %v", err)
	}

	// Create rules.star with old timestamp
	rulesPath := filepath.Join(subDir, "rules.star")
	if err := os.WriteFile(rulesPath, []byte("go_library(name = \"foo\")"), 0644); err != nil {
		t.Fatalf("Failed to write rules.star: %v", err)
	}
	oldTime := time.Now().Add(-24 * time.Hour)
	os.Chtimes(rulesPath, oldTime, oldTime)

	// Create a newer source file
	if err := os.WriteFile(filepath.Join(subDir, "foo.go"), []byte("package foo"), 0644); err != nil {
		t.Fatalf("Failed to write foo.go: %v", err)
	}

	checker := &StalenessChecker{
		ProjectRoot: tmpDir,
		UseGit:      false,
		UseMtime:    true,
		UseHash:     false,
	}

	results, err := checker.CheckDirectory(tmpDir)
	if err != nil {
		t.Fatalf("CheckDirectory failed: %v", err)
	}

	if len(results) != 1 {
		t.Errorf("Expected 1 result, got %d", len(results))
	}

	if !results[0].Stale {
		t.Error("Expected stale=true")
	}
}

func TestStalenessResult_Fields(t *testing.T) {
	result := &StalenessResult{
		Stale:        true,
		Reason:       "test reason",
		Tier:         2,
		RulesFile:    "/path/rules.star",
		SourceFiles:  []string{"main.go", "helper.go"},
		ChangedFiles: []string{"main.go"},
	}

	if !result.Stale {
		t.Error("Expected Stale=true")
	}
	if result.Reason != "test reason" {
		t.Errorf("Reason = %q, expected 'test reason'", result.Reason)
	}
	if result.Tier != 2 {
		t.Errorf("Tier = %d, expected 2", result.Tier)
	}
	if len(result.SourceFiles) != 2 {
		t.Errorf("SourceFiles length = %d, expected 2", len(result.SourceFiles))
	}
	if len(result.ChangedFiles) != 1 {
		t.Errorf("ChangedFiles length = %d, expected 1", len(result.ChangedFiles))
	}
}
