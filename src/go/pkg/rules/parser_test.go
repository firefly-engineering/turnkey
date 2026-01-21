package rules

import (
	"strings"
	"testing"
)

func TestParser_Parse_SimpleTarget(t *testing.T) {
	content := `go_binary(
    name = "myapp",
    srcs = glob(["*.go"]),
    deps = [
        "//src/go/pkg/config:config",
        "godeps//vendor/github.com/google/uuid:uuid",
    ],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if len(rf.Targets) != 1 {
		t.Fatalf("Expected 1 target, got %d", len(rf.Targets))
	}

	target := rf.Targets[0]
	if target.Name != "myapp" {
		t.Errorf("Expected target name 'myapp', got %q", target.Name)
	}
	if target.Rule != "go_binary" {
		t.Errorf("Expected rule 'go_binary', got %q", target.Rule)
	}
	if len(target.Srcs) != 1 || target.Srcs[0] != "*.go" {
		t.Errorf("Expected srcs [*.go], got %v", target.Srcs)
	}
	if len(target.Deps) != 2 {
		t.Errorf("Expected 2 deps, got %d: %v", len(target.Deps), target.Deps)
	}
}

func TestParser_Parse_MultipleTargets(t *testing.T) {
	content := `go_library(
    name = "mylib",
    srcs = glob(["*.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = glob(["*_test.go"]),
    deps = [":mylib"],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if len(rf.Targets) != 2 {
		t.Fatalf("Expected 2 targets, got %d", len(rf.Targets))
	}

	if rf.Targets[0].Name != "mylib" {
		t.Errorf("Expected first target 'mylib', got %q", rf.Targets[0].Name)
	}
	if rf.Targets[0].Rule != "go_library" {
		t.Errorf("Expected rule 'go_library', got %q", rf.Targets[0].Rule)
	}

	if rf.Targets[1].Name != "mylib_test" {
		t.Errorf("Expected second target 'mylib_test', got %q", rf.Targets[1].Name)
	}
	if rf.Targets[1].Rule != "go_test" {
		t.Errorf("Expected rule 'go_test', got %q", rf.Targets[1].Rule)
	}
}

func TestParser_Parse_WithLoadStatements(t *testing.T) {
	content := `load("@prelude//:rules.bzl", "go_binary", "go_library")
load("//custom:defs.bzl", custom_rule = "custom")

go_binary(
    name = "myapp",
    srcs = ["main.go"],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if len(rf.Loads) != 2 {
		t.Fatalf("Expected 2 load statements, got %d", len(rf.Loads))
	}

	// Check first load statement
	expected1 := `load("@prelude//:rules.bzl", "go_binary", "go_library")`
	if rf.Loads[0] != expected1 {
		t.Errorf("Expected load %q, got %q", expected1, rf.Loads[0])
	}

	// Check second load with alias
	expected2 := `load("//custom:defs.bzl", custom_rule = "custom")`
	if rf.Loads[1] != expected2 {
		t.Errorf("Expected load %q, got %q", expected2, rf.Loads[1])
	}
}

func TestParser_Parse_WithMarkers(t *testing.T) {
	content := `go_binary(
    name = "myapp",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "//src/go/pkg/foo:foo",
        "godeps//vendor/github.com/bar:bar",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//custom:dep",
        # turnkey:preserve-end
    ],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if len(rf.Targets) != 1 {
		t.Fatalf("Expected 1 target, got %d", len(rf.Targets))
	}

	target := rf.Targets[0]

	// Check all deps
	if len(target.Deps) != 3 {
		t.Errorf("Expected 3 total deps, got %d: %v", len(target.Deps), target.Deps)
	}

	// Check auto deps
	if len(target.AutoDeps) != 2 {
		t.Errorf("Expected 2 auto deps, got %d: %v", len(target.AutoDeps), target.AutoDeps)
	}

	// Check preserved deps
	if len(target.PreservedDeps) != 1 {
		t.Errorf("Expected 1 preserved dep, got %d: %v", len(target.PreservedDeps), target.PreservedDeps)
	}
	if target.PreservedDeps[0] != "//custom:dep" {
		t.Errorf("Expected preserved dep '//custom:dep', got %q", target.PreservedDeps[0])
	}
}

func TestParser_Parse_WithHash(t *testing.T) {
	content := `# Auto-managed by turnkey. Hash: abc123def456
# Manual sections marked with turnkey:preserve-start/end are not modified.

go_binary(
    name = "myapp",
    srcs = ["main.go"],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if rf.Hash != "abc123def456" {
		t.Errorf("Expected hash 'abc123def456', got %q", rf.Hash)
	}
}

func TestParser_Parse_ListSrcs(t *testing.T) {
	content := `go_binary(
    name = "myapp",
    srcs = [
        "main.go",
        "helper.go",
        "util.go",
    ],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	target := rf.Targets[0]
	expected := []string{"main.go", "helper.go", "util.go"}
	if len(target.Srcs) != len(expected) {
		t.Fatalf("Expected %d srcs, got %d: %v", len(expected), len(target.Srcs), target.Srcs)
	}
	for i, src := range expected {
		if target.Srcs[i] != src {
			t.Errorf("Expected src[%d]=%q, got %q", i, src, target.Srcs[i])
		}
	}
}

func TestParser_Parse_EmptyDeps(t *testing.T) {
	content := `go_binary(
    name = "myapp",
    srcs = ["main.go"],
    deps = [],
    visibility = ["PUBLIC"],
)
`
	parser := NewParser()
	rf, err := parser.Parse("test.star", content)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	target := rf.Targets[0]
	if len(target.Deps) != 0 {
		t.Errorf("Expected 0 deps, got %d: %v", len(target.Deps), target.Deps)
	}
	if len(target.AutoDeps) != 0 {
		t.Errorf("Expected 0 auto deps, got %d: %v", len(target.AutoDeps), target.AutoDeps)
	}
}

func TestParser_Parse_SyntaxError(t *testing.T) {
	content := `go_binary(
    name = "myapp"
    srcs = ["main.go"],  # Missing comma above
)
`
	parser := NewParser()
	_, err := parser.Parse("test.star", content)
	if err == nil {
		t.Error("Expected parse error for syntax error, got nil")
	}
}

func TestParser_GenerateTarget(t *testing.T) {
	parser := NewParser()

	target := &Target{
		Name:          "myapp",
		Rule:          "go_binary",
		Srcs:          []string{"*.go"},
		PreservedDeps: []string{"//custom:dep"},
	}

	autoDeps := []string{
		"//src/go/pkg/foo:foo",
		"godeps//vendor/github.com/bar:bar",
	}

	output := parser.GenerateTarget(target, autoDeps)

	// Check that output contains expected parts
	if !strings.Contains(output, "go_binary(") {
		t.Error("Output should contain 'go_binary('")
	}
	if !strings.Contains(output, `name = "myapp"`) {
		t.Error("Output should contain name")
	}
	if !strings.Contains(output, `srcs = glob(["*.go"])`) {
		t.Error("Output should contain srcs with glob")
	}
	if !strings.Contains(output, MarkerAutoStart) {
		t.Error("Output should contain auto-start marker")
	}
	if !strings.Contains(output, MarkerAutoEnd) {
		t.Error("Output should contain auto-end marker")
	}
	if !strings.Contains(output, MarkerPreserveStart) {
		t.Error("Output should contain preserve-start marker")
	}
	if !strings.Contains(output, MarkerPreserveEnd) {
		t.Error("Output should contain preserve-end marker")
	}
	if !strings.Contains(output, `"//custom:dep"`) {
		t.Error("Output should contain preserved dep")
	}
}

func TestParser_GenerateHeader(t *testing.T) {
	parser := NewParser()
	header := parser.GenerateHeader("abc123")

	if !strings.Contains(header, "Auto-managed by turnkey") {
		t.Error("Header should contain management notice")
	}
	if !strings.Contains(header, "Hash: abc123") {
		t.Error("Header should contain hash")
	}
}

func TestRulesFile_FindTarget(t *testing.T) {
	rf := &RulesFile{
		Targets: []*Target{
			{Name: "foo"},
			{Name: "bar"},
			{Name: "baz"},
		},
	}

	target := rf.FindTarget("bar")
	if target == nil {
		t.Fatal("Expected to find target 'bar'")
	}
	if target.Name != "bar" {
		t.Errorf("Expected target name 'bar', got %q", target.Name)
	}

	notFound := rf.FindTarget("nonexistent")
	if notFound != nil {
		t.Errorf("Expected nil for nonexistent target, got %v", notFound)
	}
}

func TestRulesFile_HasMarkers(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		expected bool
	}{
		{
			name:     "no markers",
			content:  "go_binary(name = \"foo\")",
			expected: false,
		},
		{
			name:     "has auto-start marker",
			content:  "# turnkey:auto-start",
			expected: true,
		},
		{
			name:     "has preserve-start marker",
			content:  "# turnkey:preserve-start",
			expected: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rf := &RulesFile{RawContent: tt.content}
			if got := rf.HasMarkers(); got != tt.expected {
				t.Errorf("HasMarkers() = %v, expected %v", got, tt.expected)
			}
		})
	}
}
