package rules

import (
	"strings"
	"testing"
)

func TestComputeDepsHash(t *testing.T) {
	tests := []struct {
		name     string
		deps     []string
		expected string // Only check that it's consistent
	}{
		{
			name: "empty deps",
			deps: []string{},
		},
		{
			name: "single dep",
			deps: []string{"//foo:bar"},
		},
		{
			name: "multiple deps",
			deps: []string{"//foo:bar", "//baz:qux"},
		},
		{
			name: "order independent",
			deps: []string{"//baz:qux", "//foo:bar"}, // Same as above but different order
		},
	}

	// Test that hash is consistent for same deps
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			hash1 := ComputeDepsHash(tt.deps)
			hash2 := ComputeDepsHash(tt.deps)

			if hash1 != hash2 {
				t.Errorf("Hash not consistent: %q != %q", hash1, hash2)
			}

			// Hash should be 16 characters (truncated)
			if len(hash1) != 16 {
				t.Errorf("Expected hash length 16, got %d: %q", len(hash1), hash1)
			}
		})
	}

	// Test order independence
	hash1 := ComputeDepsHash([]string{"//foo:bar", "//baz:qux"})
	hash2 := ComputeDepsHash([]string{"//baz:qux", "//foo:bar"})
	if hash1 != hash2 {
		t.Errorf("Hash should be order-independent: %q != %q", hash1, hash2)
	}

	// Test that different deps produce different hashes
	hashA := ComputeDepsHash([]string{"//foo:bar"})
	hashB := ComputeDepsHash([]string{"//foo:baz"})
	if hashA == hashB {
		t.Errorf("Different deps should produce different hashes: %q == %q", hashA, hashB)
	}
}

func TestFormatDeps(t *testing.T) {
	tests := []struct {
		name          string
		autoDeps      []string
		preservedDeps []string
		indent        string
		checkContains []string
	}{
		{
			name:          "empty deps",
			autoDeps:      []string{},
			preservedDeps: []string{},
			indent:        "    ",
			checkContains: []string{},
		},
		{
			name:          "auto deps only",
			autoDeps:      []string{"//foo:bar", "//baz:qux"},
			preservedDeps: []string{},
			indent:        "    ",
			checkContains: []string{
				MarkerAutoStart,
				`"//foo:bar"`,
				`"//baz:qux"`,
				MarkerAutoEnd,
			},
		},
		{
			name:          "preserved deps only",
			autoDeps:      []string{},
			preservedDeps: []string{"//custom:dep"},
			indent:        "    ",
			checkContains: []string{
				MarkerAutoStart,
				MarkerAutoEnd,
				MarkerPreserveStart,
				`"//custom:dep"`,
				MarkerPreserveEnd,
			},
		},
		{
			name:          "both auto and preserved",
			autoDeps:      []string{"//foo:bar"},
			preservedDeps: []string{"//custom:dep"},
			indent:        "    ",
			checkContains: []string{
				MarkerAutoStart,
				`"//foo:bar"`,
				MarkerAutoEnd,
				MarkerPreserveStart,
				`"//custom:dep"`,
				MarkerPreserveEnd,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := FormatDeps(tt.autoDeps, tt.preservedDeps, tt.indent)

			for _, expected := range tt.checkContains {
				if !strings.Contains(result, expected) {
					t.Errorf("Expected result to contain %q, got:\n%s", expected, result)
				}
			}
		})
	}
}

func TestDependencyType_String(t *testing.T) {
	tests := []struct {
		depType  DependencyType
		expected string
	}{
		{DependencyInternal, "internal"},
		{DependencyExternal, "external"},
		{DependencyStdLib, "stdlib"},
		{DependencyType(99), "unknown"},
	}

	for _, tt := range tests {
		t.Run(tt.expected, func(t *testing.T) {
			if got := tt.depType.String(); got != tt.expected {
				t.Errorf("DependencyType(%d).String() = %q, expected %q", tt.depType, got, tt.expected)
			}
		})
	}
}

func TestMarkerConstants(t *testing.T) {
	// Verify marker constants are properly defined
	tests := []struct {
		name     string
		marker   string
		contains string
	}{
		{"auto-start", MarkerAutoStart, "turnkey:auto-start"},
		{"auto-end", MarkerAutoEnd, "turnkey:auto-end"},
		{"preserve-start", MarkerPreserveStart, "turnkey:preserve-start"},
		{"preserve-end", MarkerPreserveEnd, "turnkey:preserve-end"},
		{"header", MarkerHeader, "Auto-managed by turnkey"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if !strings.Contains(tt.marker, tt.contains) {
				t.Errorf("Marker %q should contain %q", tt.marker, tt.contains)
			}
		})
	}
}

func TestTarget_Fields(t *testing.T) {
	target := &Target{
		Name:          "test",
		Rule:          "go_binary",
		Srcs:          []string{"main.go"},
		Deps:          []string{"//foo:bar"},
		AutoDeps:      []string{"//foo:bar"},
		PreservedDeps: []string{},
		StartLine:     1,
		EndLine:       10,
	}

	if target.Name != "test" {
		t.Errorf("Name = %q, expected 'test'", target.Name)
	}
	if target.Rule != "go_binary" {
		t.Errorf("Rule = %q, expected 'go_binary'", target.Rule)
	}
	if len(target.Srcs) != 1 || target.Srcs[0] != "main.go" {
		t.Errorf("Srcs = %v, expected [main.go]", target.Srcs)
	}
	if target.StartLine != 1 {
		t.Errorf("StartLine = %d, expected 1", target.StartLine)
	}
	if target.EndLine != 10 {
		t.Errorf("EndLine = %d, expected 10", target.EndLine)
	}
}

func TestImport_Fields(t *testing.T) {
	imp := Import{
		Path:       "github.com/foo/bar",
		SourceFile: "main.go",
		Line:       5,
		IsStdLib:   false,
	}

	if imp.Path != "github.com/foo/bar" {
		t.Errorf("Path = %q, expected 'github.com/foo/bar'", imp.Path)
	}
	if imp.SourceFile != "main.go" {
		t.Errorf("SourceFile = %q, expected 'main.go'", imp.SourceFile)
	}
	if imp.Line != 5 {
		t.Errorf("Line = %d, expected 5", imp.Line)
	}
	if imp.IsStdLib {
		t.Error("IsStdLib = true, expected false")
	}
}

func TestDependency_Fields(t *testing.T) {
	dep := Dependency{
		Target:     "//src/go/pkg/foo:foo",
		Type:       DependencyInternal,
		ImportPath: "github.com/org/repo/src/go/pkg/foo",
	}

	if dep.Target != "//src/go/pkg/foo:foo" {
		t.Errorf("Target = %q, expected '//src/go/pkg/foo:foo'", dep.Target)
	}
	if dep.Type != DependencyInternal {
		t.Errorf("Type = %v, expected DependencyInternal", dep.Type)
	}
	if dep.ImportPath != "github.com/org/repo/src/go/pkg/foo" {
		t.Errorf("ImportPath = %q, expected 'github.com/org/repo/src/go/pkg/foo'", dep.ImportPath)
	}
}

func TestRulesFile_Fields(t *testing.T) {
	rf := &RulesFile{
		Path:       "/path/to/rules.star",
		Loads:      []string{`load("@prelude//:rules.bzl", "go_binary")`},
		Targets:    []*Target{{Name: "test"}},
		Hash:       "abc123",
		RawContent: "content",
	}

	if rf.Path != "/path/to/rules.star" {
		t.Errorf("Path = %q, expected '/path/to/rules.star'", rf.Path)
	}
	if len(rf.Loads) != 1 {
		t.Errorf("Loads length = %d, expected 1", len(rf.Loads))
	}
	if len(rf.Targets) != 1 {
		t.Errorf("Targets length = %d, expected 1", len(rf.Targets))
	}
	if rf.Hash != "abc123" {
		t.Errorf("Hash = %q, expected 'abc123'", rf.Hash)
	}
	if rf.RawContent != "content" {
		t.Errorf("RawContent = %q, expected 'content'", rf.RawContent)
	}
}
