package mapper

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

func TestMapGoImports(t *testing.T) {
	m := &Mapper{
		config: Config{
			Go: &GoConfig{
				ModulePath:   "github.com/firefly-engineering/turnkey",
				ExternalCell: "godeps",
				ExternalDeps: map[string]bool{
					"github.com/google/uuid":  true,
					"golang.org/x/sys":        true,
					"go.starlark.net":         true,
				},
			},
		},
	}

	tests := []struct {
		name     string
		imports  []extraction.Import
		wantDeps []string
	}{
		{
			name: "stdlib only",
			imports: []extraction.Import{
				{Path: "fmt", Kind: extraction.ImportKindStdlib},
				{Path: "os", Kind: extraction.ImportKindStdlib},
			},
			wantDeps: nil, // stdlib should be skipped
		},
		{
			name: "internal import",
			imports: []extraction.Import{
				{Path: "github.com/firefly-engineering/turnkey/src/go/pkg/foo", Kind: extraction.ImportKindInternal},
			},
			wantDeps: []string{"//src/go/pkg/foo:foo"},
		},
		{
			name: "external import",
			imports: []extraction.Import{
				{Path: "github.com/google/uuid", Kind: extraction.ImportKindExternal},
			},
			wantDeps: []string{"godeps//vendor/github.com/google/uuid:uuid"},
		},
		{
			name: "external subpackage",
			imports: []extraction.Import{
				{Path: "golang.org/x/sys/cpu", Kind: extraction.ImportKindExternal},
			},
			wantDeps: []string{"godeps//vendor/golang.org/x/sys/cpu:cpu"},
		},
		{
			name: "mixed imports",
			imports: []extraction.Import{
				{Path: "fmt", Kind: extraction.ImportKindStdlib},
				{Path: "github.com/firefly-engineering/turnkey/src/go/pkg/bar", Kind: extraction.ImportKindInternal},
				{Path: "go.starlark.net/syntax", Kind: extraction.ImportKindExternal},
			},
			wantDeps: []string{
				"//src/go/pkg/bar:bar",
				"godeps//vendor/go.starlark.net/syntax:syntax",
			},
		},
		{
			name: "deduplication",
			imports: []extraction.Import{
				{Path: "github.com/google/uuid", Kind: extraction.ImportKindExternal},
				{Path: "github.com/google/uuid", Kind: extraction.ImportKindExternal},
			},
			wantDeps: []string{"godeps//vendor/github.com/google/uuid:uuid"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			deps, _ := m.mapGoImports(tt.imports)
			targets := DepsToTargets(deps)

			if len(targets) != len(tt.wantDeps) {
				t.Errorf("got %d deps, want %d: %v", len(targets), len(tt.wantDeps), targets)
				return
			}

			for i, want := range tt.wantDeps {
				if targets[i] != want {
					t.Errorf("deps[%d] = %q, want %q", i, targets[i], want)
				}
			}
		})
	}
}

func TestMapExtractionResult(t *testing.T) {
	m := &Mapper{
		config: Config{
			Go: &GoConfig{
				ModulePath:   "github.com/example/project",
				ExternalCell: "godeps",
				ExternalDeps: map[string]bool{
					"github.com/google/uuid": true,
				},
			},
		},
	}

	result := &extraction.Result{
		Version:  "1",
		Language: "go",
		Packages: []extraction.Package{
			{
				Path:  "src/cmd/myapp",
				Files: []string{"main.go"},
				Imports: []extraction.Import{
					{Path: "fmt", Kind: extraction.ImportKindStdlib},
					{Path: "github.com/example/project/src/go/pkg/lib", Kind: extraction.ImportKindInternal},
					{Path: "github.com/google/uuid", Kind: extraction.ImportKindExternal},
				},
				TestImports: []extraction.Import{
					{Path: "testing", Kind: extraction.ImportKindStdlib},
				},
			},
		},
	}

	mappings, err := m.MapExtractionResult(result)
	if err != nil {
		t.Fatalf("MapExtractionResult failed: %v", err)
	}

	if len(mappings) != 1 {
		t.Fatalf("expected 1 mapping, got %d", len(mappings))
	}

	mapping := mappings["src/cmd/myapp"]
	if len(mapping.Deps) != 2 {
		t.Errorf("expected 2 deps, got %d", len(mapping.Deps))
	}

	// Should have internal and external deps (stdlib skipped)
	targets := DepsToTargets(mapping.Deps)
	hasInternal := false
	hasExternal := false
	for _, target := range targets {
		if target == "//src/go/pkg/lib:lib" {
			hasInternal = true
		}
		if target == "godeps//vendor/github.com/google/uuid:uuid" {
			hasExternal = true
		}
	}

	if !hasInternal {
		t.Error("missing internal dep")
	}
	if !hasExternal {
		t.Error("missing external dep")
	}
}

func TestUnmappedExternalDep(t *testing.T) {
	m := &Mapper{
		config: Config{
			Go: &GoConfig{
				ModulePath:   "github.com/example/project",
				ExternalCell: "godeps",
				ExternalDeps: map[string]bool{
					// Only uuid is known
					"github.com/google/uuid": true,
				},
			},
		},
	}

	imports := []extraction.Import{
		{Path: "github.com/unknown/package", Kind: extraction.ImportKindExternal},
	}

	deps, unmapped := m.mapGoImports(imports)

	if len(deps) != 0 {
		t.Errorf("expected 0 deps for unknown import, got %d", len(deps))
	}

	if len(unmapped) != 1 {
		t.Errorf("expected 1 unmapped, got %d", len(unmapped))
	}

	if unmapped[0] != "github.com/unknown/package" {
		t.Errorf("unmapped = %q, want github.com/unknown/package", unmapped[0])
	}
}

func TestExtractModulePath(t *testing.T) {
	tests := []struct {
		content string
		want    string
	}{
		{
			content: "module github.com/foo/bar\n\ngo 1.21\n",
			want:    "github.com/foo/bar",
		},
		{
			content: "module github.com/firefly-engineering/turnkey",
			want:    "github.com/firefly-engineering/turnkey",
		},
		{
			content: "// comment\nmodule example.com/pkg\n",
			want:    "example.com/pkg",
		},
		{
			content: "go 1.21\n", // no module line
			want:    "",
		},
	}

	for _, tt := range tests {
		got := extractModulePath(tt.content)
		if got != tt.want {
			t.Errorf("extractModulePath(%q) = %q, want %q", tt.content, got, tt.want)
		}
	}
}

func TestApplyToRulesStar(t *testing.T) {
	// Create a temp directory
	dir := t.TempDir()

	// Create a test rules.star
	rulesContent := `load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "mylib",
    srcs = ["lib.go"],
    deps = [
        "//old:dep",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = ["lib_test.go"],
    target_under_test = ":mylib",
    deps = [],
    visibility = ["PUBLIC"],
)
`
	rulesPath := filepath.Join(dir, "mylib", "rules.star")
	if err := os.MkdirAll(filepath.Dir(rulesPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(rulesPath, []byte(rulesContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create mapper and apply
	m := &Mapper{config: Config{}}

	mapping := PackageMapping{
		Path: "mylib",
		Deps: []MappedDep{
			{Target: "//new:dep1", Type: DependencyInternal},
			{Target: "godeps//vendor/github.com/foo:foo", Type: DependencyExternal},
		},
		TestDeps: []MappedDep{
			{Target: "//test:only", Type: DependencyInternal},
		},
	}

	if err := m.ApplyToRulesStar(rulesPath, mapping); err != nil {
		t.Fatalf("ApplyToRulesStar failed: %v", err)
	}

	// Read back and verify
	content, err := os.ReadFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}

	result := string(content)

	// Check library deps were updated
	if !contains(result, `"//new:dep1"`) {
		t.Error("missing //new:dep1 in output")
	}
	if !contains(result, `"godeps//vendor/github.com/foo:foo"`) {
		t.Error("missing godeps dep in output")
	}
	if contains(result, `"//old:dep"`) {
		t.Error("old dep should have been replaced")
	}
}

func contains(s, substr string) bool {
	return len(s) > 0 && len(substr) > 0 && (s == substr || len(s) > len(substr) && (s[:len(substr)] == substr || s[len(s)-len(substr):] == substr || containsInMiddle(s, substr)))
}

func containsInMiddle(s, substr string) bool {
	for i := 1; i < len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
