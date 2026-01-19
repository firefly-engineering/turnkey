package staleness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestParseBuckSrcs_SingleLine(t *testing.T) {
	dir := t.TempDir()
	buckFile := filepath.Join(dir, "rules.star")

	content := `load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = ["main.go", "helper.go"],
    visibility = ["PUBLIC"],
)
`
	if err := os.WriteFile(buckFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	srcs, err := parseBuckSrcs(buckFile, "go_library")
	if err != nil {
		t.Fatal(err)
	}

	expected := []string{"main.go", "helper.go"}
	if len(srcs) != len(expected) {
		t.Fatalf("expected %d srcs, got %d: %v", len(expected), len(srcs), srcs)
	}

	for i, s := range srcs {
		if s != expected[i] {
			t.Errorf("expected srcs[%d]=%s, got %s", i, expected[i], s)
		}
	}
}

func TestParseBuckSrcs_MultiLine(t *testing.T) {
	dir := t.TempDir()
	buckFile := filepath.Join(dir, "rules.star")

	content := `load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "mylib",
    package_name = "github.com/example/mylib",
    srcs = [
        "file1.go",
        "file2.go",
        "file3.go",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = ["mylib_test.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	srcs, err := parseBuckSrcs(buckFile, "go_library")
	if err != nil {
		t.Fatal(err)
	}

	expected := []string{"file1.go", "file2.go", "file3.go"}
	if len(srcs) != len(expected) {
		t.Fatalf("expected %d srcs, got %d: %v", len(expected), len(srcs), srcs)
	}

	for i, s := range srcs {
		if s != expected[i] {
			t.Errorf("expected srcs[%d]=%s, got %s", i, expected[i], s)
		}
	}
}

func TestParseBuckSrcs_NoRule(t *testing.T) {
	dir := t.TempDir()
	buckFile := filepath.Join(dir, "rules.star")

	content := `load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = ["main.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// Looking for go_test which doesn't exist
	srcs, err := parseBuckSrcs(buckFile, "go_test")
	if err != nil {
		t.Fatal(err)
	}

	if srcs != nil {
		t.Errorf("expected nil for missing rule, got %v", srcs)
	}
}

func TestGlobGoSrcs(t *testing.T) {
	dir := t.TempDir()

	// Create Go source files
	files := map[string]bool{
		"main.go":     false, // not a test
		"helper.go":   false,
		"main_test.go": true, // is a test
		"readme.md":   false, // not Go
	}

	for name := range files {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("package main"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	// Test non-test files
	srcs, err := globGoSrcs(dir, false)
	if err != nil {
		t.Fatal(err)
	}

	if len(srcs) != 2 {
		t.Errorf("expected 2 non-test Go files, got %d: %v", len(srcs), srcs)
	}

	// Test test files
	testSrcs, err := globGoSrcs(dir, true)
	if err != nil {
		t.Fatal(err)
	}

	if len(testSrcs) != 1 {
		t.Errorf("expected 1 test file, got %d: %v", len(testSrcs), testSrcs)
	}
	if testSrcs[0] != "main_test.go" {
		t.Errorf("expected main_test.go, got %s", testSrcs[0])
	}
}

func TestCheckGoSrcList_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    srcs = ["main.go", "helper.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create matching Go files
	for _, name := range []string{"main.go", "helper.go"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("package lib"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckGoSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckGoSrcList_ExtraFile(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file with only main.go
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    srcs = ["main.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create main.go and an extra file
	for _, name := range []string{"main.go", "extra.go"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("package lib"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckGoSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to extra file")
	}

	if len(result.Extra) != 1 || result.Extra[0] != "extra.go" {
		t.Errorf("expected extra=[extra.go], got %v", result.Extra)
	}
}

func TestCheckGoSrcList_MissingFile(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file declaring files
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    srcs = ["main.go", "missing.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Only create main.go
	path := filepath.Join(dir, "main.go")
	if err := os.WriteFile(path, []byte("package lib"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckGoSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to missing file")
	}

	if len(result.Missing) != 1 || result.Missing[0] != "missing.go" {
		t.Errorf("expected missing=[missing.go], got %v", result.Missing)
	}
}

func TestCheckGoTestSrcList(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file with go_test
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    srcs = ["main.go"],
)

go_test(
    name = "lib_test",
    srcs = ["lib_test.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create files
	for _, name := range []string{"main.go", "lib_test.go", "extra_test.go"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("package lib"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckGoTestSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to extra test file")
	}

	if len(result.Extra) != 1 || result.Extra[0] != "extra_test.go" {
		t.Errorf("expected extra=[extra_test.go], got %v", result.Extra)
	}
}
