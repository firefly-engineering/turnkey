package staleness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestGlobPythonSrcs(t *testing.T) {
	dir := t.TempDir()

	// Create Python source files
	files := []string{
		"main.py",
		"helper.py",
		"test_main.py",    // test file (prefix)
		"helper_test.py",  // test file (suffix)
	}

	for _, name := range files {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("# Python"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	// Test non-test files
	srcs, err := globPythonSrcs(dir, false)
	if err != nil {
		t.Fatal(err)
	}

	if len(srcs) != 2 {
		t.Errorf("expected 2 non-test files, got %d: %v", len(srcs), srcs)
	}

	// Test test files
	testSrcs, err := globPythonSrcs(dir, true)
	if err != nil {
		t.Fatal(err)
	}

	if len(testSrcs) != 2 {
		t.Errorf("expected 2 test files, got %d: %v", len(testSrcs), testSrcs)
	}
}

func TestParsePythonImports(t *testing.T) {
	dir := t.TempDir()

	// Create a Python source file
	pyFile := filepath.Join(dir, "main.py")
	content := `import os
import sys
import json
from typing import List
from requests import Session
from mypackage.module import helper

# This is a comment
import argparse

def main():
    pass
`
	if err := os.WriteFile(pyFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	imports, err := parsePythonImports(dir)
	if err != nil {
		t.Fatal(err)
	}

	expected := map[string]bool{
		"os":        true,
		"sys":       true,
		"json":      true,
		"typing":    true,
		"requests":  true,
		"mypackage": true,
		"argparse":  true,
	}

	if len(imports) != len(expected) {
		t.Errorf("expected %d imports, got %d: %v", len(expected), len(imports), imports)
	}

	for _, imp := range imports {
		if !expected[imp] {
			t.Errorf("unexpected import: %s", imp)
		}
	}
}

func TestFilterExternalPythonImports(t *testing.T) {
	imports := []string{
		"os",
		"sys",
		"json",
		"requests",
		"flask",
		"typing",
	}

	external := filterExternalPythonImports(imports)

	// Should only have requests and flask
	if len(external) != 2 {
		t.Errorf("expected 2 external imports, got %d: %v", len(external), external)
	}

	expectedExternal := map[string]bool{"requests": true, "flask": true}
	for _, imp := range external {
		if !expectedExternal[imp] {
			t.Errorf("unexpected external import: %s", imp)
		}
	}
}

func TestExtractPythonDepPkg(t *testing.T) {
	tests := []struct {
		dep string
		pkg string
	}{
		{"pydeps//vendor/requests:requests", "requests"},
		{"pydeps//vendor/flask:flask", "flask"},
		{"pydeps//vendor/boto3:boto3", "boto3"},
		{"//lib/mylib:mylib", ""},
		{":local", ""},
	}

	for _, tc := range tests {
		got := extractPythonDepPkg(tc.dep)
		if got != tc.pkg {
			t.Errorf("extractPythonDepPkg(%q) = %q, want %q", tc.dep, got, tc.pkg)
		}
	}
}

func TestCheckPythonSrcList_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `python_library(
    name = "mylib",
    srcs = ["main.py", "helper.py"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create matching Python files
	for _, name := range []string{"main.py", "helper.py"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("# Python"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckPythonSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckPythonSrcList_ExtraFile(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `python_library(
    name = "mylib",
    srcs = ["main.py"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create main.py and an extra file
	for _, name := range []string{"main.py", "extra.py"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("# Python"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckPythonSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to extra file")
	}

	if len(result.Extra) != 1 || result.Extra[0] != "extra.py" {
		t.Errorf("expected extra=[extra.py], got %v", result.Extra)
	}
}

func TestCheckPythonImports_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `python_library(
    name = "mylib",
    srcs = ["main.py"],
    deps = [
        "pydeps//vendor/requests:requests",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Python file with matching import
	pyFile := filepath.Join(dir, "main.py")
	pyContent := `import os
import requests

def main():
    requests.get("http://example.com")
`
	if err := os.WriteFile(pyFile, []byte(pyContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckPythonImports(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckPythonImports_MissingDep(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file without deps
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `python_library(
    name = "mylib",
    srcs = ["main.py"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Python file with external import
	pyFile := filepath.Join(dir, "main.py")
	pyContent := `import requests

def main():
    requests.get("http://example.com")
`
	if err := os.WriteFile(pyFile, []byte(pyContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckPythonImports(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to missing dep")
	}

	if len(result.Missing) != 1 || result.Missing[0] != "requests" {
		t.Errorf("expected missing=[requests], got %v", result.Missing)
	}
}

func TestCachedCheckPythonPackage(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `python_library(
    name = "lib",
    srcs = ["main.py"],
    deps = [
        "pydeps//vendor/requests:requests",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Python file
	pyFile := filepath.Join(dir, "main.py")
	pyContent := `import requests

def main():
    pass
`
	if err := os.WriteFile(pyFile, []byte(pyContent), 0644); err != nil {
		t.Fatal(err)
	}

	cache := NewCache()
	cc := &CachedCheck{Cache: cache, BuckFile: buckFile}

	// First check should not be from cache
	result1, err := cc.CheckPythonPackage()
	if err != nil {
		t.Fatal(err)
	}

	if result1.FromCache {
		t.Error("expected first check not from cache")
	}

	// Second check should be from cache
	result2, err := cc.CheckPythonPackage()
	if err != nil {
		t.Fatal(err)
	}

	if !result2.FromCache {
		t.Error("expected second check from cache")
	}
}
