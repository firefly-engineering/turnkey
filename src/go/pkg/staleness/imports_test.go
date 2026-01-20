package staleness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestParseBuckDeps(t *testing.T) {
	dir := t.TempDir()
	buckFile := filepath.Join(dir, "rules.star")

	content := `load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    package_name = "github.com/example/mylib",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
        "//go/pkg/other:other",
    ],
    visibility = ["PUBLIC"],
)
`
	if err := os.WriteFile(buckFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	deps, err := parseBuckDeps(buckFile, "go_library")
	if err != nil {
		t.Fatal(err)
	}

	expected := []string{
		"godeps//vendor/github.com/google/uuid:uuid",
		"//go/pkg/other:other",
	}
	if len(deps) != len(expected) {
		t.Fatalf("expected %d deps, got %d: %v", len(expected), len(deps), deps)
	}

	for i, d := range deps {
		if d != expected[i] {
			t.Errorf("expected deps[%d]=%s, got %s", i, expected[i], d)
		}
	}
}

func TestParseBuckPackageName(t *testing.T) {
	dir := t.TempDir()
	buckFile := filepath.Join(dir, "rules.star")

	content := `go_library(
    name = "mylib",
    package_name = "github.com/example/mylib",
    srcs = ["main.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	pkgName, err := parseBuckPackageName(buckFile, "go_library")
	if err != nil {
		t.Fatal(err)
	}

	if pkgName != "github.com/example/mylib" {
		t.Errorf("expected github.com/example/mylib, got %s", pkgName)
	}
}

func TestParseGoImports(t *testing.T) {
	dir := t.TempDir()

	// Create a Go source file
	goFile := filepath.Join(dir, "main.go")
	content := `package main

import (
	"fmt"
	"os"

	"github.com/google/uuid"
	"github.com/example/mylib/internal"
)

func main() {
	fmt.Println(uuid.New(), os.Args)
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	imports, err := parseGoImports(dir, false)
	if err != nil {
		t.Fatal(err)
	}

	expected := []string{
		"fmt",
		"github.com/example/mylib/internal",
		"github.com/google/uuid",
		"os",
	}

	if len(imports) != len(expected) {
		t.Fatalf("expected %d imports, got %d: %v", len(expected), len(imports), imports)
	}

	for i, imp := range imports {
		if imp != expected[i] {
			t.Errorf("expected imports[%d]=%s, got %s", i, expected[i], imp)
		}
	}
}

func TestFilterExternalImports(t *testing.T) {
	imports := []string{
		"fmt",
		"os",
		"github.com/google/uuid",
		"github.com/example/mylib/internal",
	}

	// Filter with self-package
	external := filterExternalImports(imports, "github.com/example/mylib")

	// Should have only uuid (fmt, os are stdlib, internal is self-reference)
	if len(external) != 1 {
		t.Fatalf("expected 1 external import, got %d: %v", len(external), external)
	}

	if external[0] != "github.com/google/uuid" {
		t.Errorf("expected github.com/google/uuid, got %s", external[0])
	}
}

func TestIsStdLib(t *testing.T) {
	tests := []struct {
		path   string
		stdlib bool
	}{
		{"fmt", true},
		{"os", true},
		{"os/exec", true},
		{"net/http", true},
		{"encoding/json", true},
		{"github.com/foo/bar", false},
		{"golang.org/x/tools", false},
		{"example.com/pkg", false},
	}

	for _, tc := range tests {
		got := isStdLib(tc.path)
		if got != tc.stdlib {
			t.Errorf("isStdLib(%q) = %v, want %v", tc.path, got, tc.stdlib)
		}
	}
}

func TestExtractDepPath(t *testing.T) {
	tests := []struct {
		dep  string
		path string
	}{
		{"godeps//vendor/github.com/google/uuid:uuid", "github.com/google/uuid"},
		{"godeps//vendor/golang.org/x/sync/errgroup:errgroup", "golang.org/x/sync/errgroup"},
		{"//go/pkg/mylib:mylib", ""},
		{":local", ""},
	}

	for _, tc := range tests {
		got := extractDepPath(tc.dep)
		if got != tc.path {
			t.Errorf("extractDepPath(%q) = %q, want %q", tc.dep, got, tc.path)
		}
	}
}

func TestCheckGoImports_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    package_name = "github.com/example/mylib",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Go file with matching import
	goFile := filepath.Join(dir, "main.go")
	goContent := `package mylib

import (
	"fmt"
	"github.com/google/uuid"
)

func Do() {
	fmt.Println(uuid.New())
}
`
	if err := os.WriteFile(goFile, []byte(goContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckGoImports(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckGoImports_MissingDep(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file without deps
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    package_name = "github.com/example/mylib",
    srcs = ["main.go"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Go file with external import
	goFile := filepath.Join(dir, "main.go")
	goContent := `package mylib

import (
	"github.com/google/uuid"
)

func Do() string {
	return uuid.New().String()
}
`
	if err := os.WriteFile(goFile, []byte(goContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckGoImports(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to missing dep")
	}

	if len(result.Missing) != 1 || result.Missing[0] != "github.com/google/uuid" {
		t.Errorf("expected missing=[github.com/google/uuid], got %v", result.Missing)
	}
}

func TestCheckGoImports_ExtraDep(t *testing.T) {
	dir := t.TempDir()

	// Create rules.star file with deps
	buckFile := filepath.Join(dir, "rules.star")
	buckContent := `go_library(
    name = "lib",
    package_name = "github.com/example/mylib",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
        "godeps//vendor/github.com/unused/pkg:pkg",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Go file that only uses uuid
	goFile := filepath.Join(dir, "main.go")
	goContent := `package mylib

import (
	"github.com/google/uuid"
)

func Do() string {
	return uuid.New().String()
}
`
	if err := os.WriteFile(goFile, []byte(goContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckGoImports(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to extra dep")
	}

	if len(result.Extra) != 1 {
		t.Errorf("expected 1 extra dep, got %v", result.Extra)
	}
}
