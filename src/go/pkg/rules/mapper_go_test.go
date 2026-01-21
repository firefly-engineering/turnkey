package rules

import (
	"os"
	"path/filepath"
	"testing"
)

func TestGoMapper_MapImport_StdLib(t *testing.T) {
	mapper := &GoMapper{
		ModulePath:   "github.com/test/repo",
		ExternalDeps: make(map[string]GoDepsEntry),
	}

	imp := Import{
		Path:     "fmt",
		IsStdLib: true,
	}

	dep := mapper.MapImport(imp)
	if dep == nil {
		t.Fatal("Expected non-nil dependency for stdlib")
	}
	if dep.Type != DependencyStdLib {
		t.Errorf("Expected DependencyStdLib, got %v", dep.Type)
	}
	if dep.ImportPath != "fmt" {
		t.Errorf("ImportPath = %q, expected 'fmt'", dep.ImportPath)
	}
}

func TestGoMapper_MapImport_Internal(t *testing.T) {
	mapper := &GoMapper{
		ProjectRoot:    "/project",
		ModulePath:     "github.com/test/repo",
		InternalPrefix: "//src/go",
		ExternalDeps:   make(map[string]GoDepsEntry),
	}

	imp := Import{
		Path:     "github.com/test/repo/src/go/pkg/foo",
		IsStdLib: false,
	}

	dep := mapper.MapImport(imp)
	if dep == nil {
		t.Fatal("Expected non-nil dependency for internal import")
	}
	if dep.Type != DependencyInternal {
		t.Errorf("Expected DependencyInternal, got %v", dep.Type)
	}
	if dep.Target != "//src/go/pkg/foo:foo" {
		t.Errorf("Target = %q, expected '//src/go/pkg/foo:foo'", dep.Target)
	}
}

func TestGoMapper_MapImport_External(t *testing.T) {
	mapper := &GoMapper{
		ProjectRoot:    "/project",
		ModulePath:     "github.com/test/repo",
		InternalPrefix: "//src/go",
		ExternalCell:   "godeps",
		ExternalDeps: map[string]GoDepsEntry{
			"github.com/google/uuid": {Version: "v1.6.0"},
		},
	}

	imp := Import{
		Path:     "github.com/google/uuid",
		IsStdLib: false,
	}

	dep := mapper.MapImport(imp)
	if dep == nil {
		t.Fatal("Expected non-nil dependency for external import")
	}
	if dep.Type != DependencyExternal {
		t.Errorf("Expected DependencyExternal, got %v", dep.Type)
	}
	if dep.Target != "godeps//vendor/github.com/google/uuid:uuid" {
		t.Errorf("Target = %q, expected 'godeps//vendor/github.com/google/uuid:uuid'", dep.Target)
	}
}

func TestGoMapper_MapImport_ExternalSubPackage(t *testing.T) {
	mapper := &GoMapper{
		ProjectRoot:    "/project",
		ModulePath:     "github.com/test/repo",
		InternalPrefix: "//src/go",
		ExternalCell:   "godeps",
		ExternalDeps: map[string]GoDepsEntry{
			"golang.org/x/sys": {Version: "v0.40.0"},
		},
	}

	// Import a subpackage of a registered dep
	imp := Import{
		Path:     "golang.org/x/sys/cpu",
		IsStdLib: false,
	}

	dep := mapper.MapImport(imp)
	if dep == nil {
		t.Fatal("Expected non-nil dependency for external subpackage import")
	}
	if dep.Type != DependencyExternal {
		t.Errorf("Expected DependencyExternal, got %v", dep.Type)
	}
	// Should use the full import path, not the root dep
	if dep.Target != "godeps//vendor/golang.org/x/sys/cpu:cpu" {
		t.Errorf("Target = %q, expected 'godeps//vendor/golang.org/x/sys/cpu:cpu'", dep.Target)
	}
}

func TestGoMapper_MapImport_Unknown(t *testing.T) {
	mapper := &GoMapper{
		ProjectRoot:    "/project",
		ModulePath:     "github.com/test/repo",
		InternalPrefix: "//src/go",
		ExternalCell:   "godeps",
		ExternalDeps:   make(map[string]GoDepsEntry),
	}

	// Import something not in deps and not internal
	imp := Import{
		Path:     "github.com/unknown/package",
		IsStdLib: false,
	}

	dep := mapper.MapImport(imp)
	if dep != nil {
		t.Errorf("Expected nil for unknown import, got %v", dep)
	}
}

func TestGoMapper_IsInternal(t *testing.T) {
	mapper := &GoMapper{
		ModulePath: "github.com/test/repo",
	}

	tests := []struct {
		path     string
		expected bool
	}{
		{"github.com/test/repo/pkg/foo", true},
		{"github.com/test/repo", true}, // Exact match
		{"github.com/other/repo", false},
		{"github.com/test/otherrepo", false},
		{"fmt", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if got := mapper.isInternal(tt.path); got != tt.expected {
				t.Errorf("isInternal(%q) = %v, expected %v", tt.path, got, tt.expected)
			}
		})
	}

	// Test with empty module path
	emptyMapper := &GoMapper{}
	if emptyMapper.isInternal("github.com/test/repo/pkg") {
		t.Error("isInternal should return false when ModulePath is empty")
	}
}

func TestGoMapper_IsExternal(t *testing.T) {
	mapper := &GoMapper{
		ExternalDeps: map[string]GoDepsEntry{
			"github.com/google/uuid": {},
			"golang.org/x/sys":       {},
		},
	}

	tests := []struct {
		path     string
		expected bool
	}{
		{"github.com/google/uuid", true},       // Exact match
		{"golang.org/x/sys", true},             // Exact match
		{"golang.org/x/sys/cpu", true},         // Subpackage
		{"golang.org/x/sys/unix", true},        // Another subpackage
		{"github.com/unknown/pkg", false},      // Not in deps
		{"github.com/google/other", false},     // Different package
		{"github.com/google/uuidv2", false},    // Not a subpackage
		{"fmt", false},                         // Stdlib
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if got := mapper.isExternal(tt.path); got != tt.expected {
				t.Errorf("isExternal(%q) = %v, expected %v", tt.path, got, tt.expected)
			}
		})
	}
}

func TestGoMapper_MapImports(t *testing.T) {
	mapper := &GoMapper{
		ProjectRoot:    "/project",
		ModulePath:     "github.com/test/repo",
		InternalPrefix: "//src/go",
		ExternalCell:   "godeps",
		ExternalDeps: map[string]GoDepsEntry{
			"github.com/google/uuid": {Version: "v1.6.0"},
		},
	}

	imports := []Import{
		{Path: "fmt", IsStdLib: true},
		{Path: "github.com/test/repo/pkg/foo", IsStdLib: false},
		{Path: "github.com/google/uuid", IsStdLib: false},
		{Path: "github.com/unknown/pkg", IsStdLib: false},
		{Path: "github.com/google/uuid", IsStdLib: false}, // Duplicate
	}

	deps, unmapped := mapper.MapImports(imports)

	// Should have 2 deps (internal + external, deduplicated)
	if len(deps) != 2 {
		t.Errorf("Expected 2 deps, got %d: %v", len(deps), deps)
	}

	// Should have 1 unmapped
	if len(unmapped) != 1 {
		t.Errorf("Expected 1 unmapped, got %d: %v", len(unmapped), unmapped)
	}
	if unmapped[0].Path != "github.com/unknown/pkg" {
		t.Errorf("Expected unmapped 'github.com/unknown/pkg', got %q", unmapped[0].Path)
	}
}

func TestGoMapper_FindRootDep(t *testing.T) {
	mapper := &GoMapper{
		ExternalDeps: map[string]GoDepsEntry{
			"golang.org/x/sys":   {},
			"github.com/foo/bar": {},
		},
	}

	tests := []struct {
		path     string
		expected string
	}{
		{"golang.org/x/sys", "golang.org/x/sys"},
		{"golang.org/x/sys/cpu", "golang.org/x/sys"},
		{"golang.org/x/sys/unix", "golang.org/x/sys"},
		{"github.com/foo/bar", "github.com/foo/bar"},
		{"github.com/foo/bar/sub", "github.com/foo/bar"},
		{"github.com/unknown/pkg", ""},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if got := mapper.findRootDep(tt.path); got != tt.expected {
				t.Errorf("findRootDep(%q) = %q, expected %q", tt.path, got, tt.expected)
			}
		})
	}
}

func TestDepsToTargets(t *testing.T) {
	deps := []Dependency{
		{Target: "//foo:bar"},
		{Target: "//baz:qux"},
		{Target: "godeps//vendor/github.com/x/y:y"},
	}

	targets := DepsToTargets(deps)

	if len(targets) != 3 {
		t.Fatalf("Expected 3 targets, got %d", len(targets))
	}

	expected := []string{"//foo:bar", "//baz:qux", "godeps//vendor/github.com/x/y:y"}
	for i, target := range targets {
		if target != expected[i] {
			t.Errorf("Target[%d] = %q, expected %q", i, target, expected[i])
		}
	}
}

func TestGoMapper_SetInternalPrefix(t *testing.T) {
	mapper := &GoMapper{InternalPrefix: "//default"}

	mapper.SetInternalPrefix("//custom")
	if mapper.InternalPrefix != "//custom" {
		t.Errorf("InternalPrefix = %q, expected '//custom'", mapper.InternalPrefix)
	}
}

func TestGoMapper_SetExternalCell(t *testing.T) {
	mapper := &GoMapper{ExternalCell: "default"}

	mapper.SetExternalCell("custom")
	if mapper.ExternalCell != "custom" {
		t.Errorf("ExternalCell = %q, expected 'custom'", mapper.ExternalCell)
	}
}

func TestNewGoMapper(t *testing.T) {
	// Create a temporary directory with go.mod and go-deps.toml
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

	// Create go-deps.toml
	goDeps := filepath.Join(tmpDir, "go-deps.toml")
	depsContent := `schema_version = 1

[deps."github.com/google/uuid"]
version = "v1.6.0"
hash = "sha256-abc123"
`
	if err := os.WriteFile(goDeps, []byte(depsContent), 0644); err != nil {
		t.Fatalf("Failed to write go-deps.toml: %v", err)
	}

	mapper, err := NewGoMapper(tmpDir)
	if err != nil {
		t.Fatalf("NewGoMapper failed: %v", err)
	}

	if mapper.ProjectRoot != tmpDir {
		t.Errorf("ProjectRoot = %q, expected %q", mapper.ProjectRoot, tmpDir)
	}

	if mapper.ModulePath != "github.com/test/repo" {
		t.Errorf("ModulePath = %q, expected 'github.com/test/repo'", mapper.ModulePath)
	}

	if mapper.InternalPrefix != "//src/go" {
		t.Errorf("InternalPrefix = %q, expected '//src/go'", mapper.InternalPrefix)
	}

	if mapper.ExternalCell != "godeps" {
		t.Errorf("ExternalCell = %q, expected 'godeps'", mapper.ExternalCell)
	}

	// Check that deps were loaded
	if _, ok := mapper.ExternalDeps["github.com/google/uuid"]; !ok {
		t.Error("Expected 'github.com/google/uuid' in ExternalDeps")
	}
}

func TestNewGoMapper_NoFiles(t *testing.T) {
	// Create an empty temporary directory
	tmpDir, err := os.MkdirTemp("", "rules-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	mapper, err := NewGoMapper(tmpDir)
	if err != nil {
		t.Fatalf("NewGoMapper failed: %v", err)
	}

	// Should still create mapper with defaults
	if mapper == nil {
		t.Fatal("Expected non-nil mapper")
	}

	if mapper.ModulePath != "" {
		t.Errorf("ModulePath should be empty, got %q", mapper.ModulePath)
	}

	if len(mapper.ExternalDeps) != 0 {
		t.Errorf("ExternalDeps should be empty, got %v", mapper.ExternalDeps)
	}
}

func TestGoDepsFile_Structure(t *testing.T) {
	depsFile := GoDepsFile{
		SchemaVersion: 1,
		Deps: map[string]GoDepsEntry{
			"github.com/foo/bar": {
				Version:  "v1.0.0",
				Hash:     "sha256-abc",
				Indirect: false,
			},
			"github.com/baz/qux": {
				Version:  "v2.0.0",
				Hash:     "sha256-def",
				Indirect: true,
			},
		},
	}

	if depsFile.SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, expected 1", depsFile.SchemaVersion)
	}

	if len(depsFile.Deps) != 2 {
		t.Errorf("Deps count = %d, expected 2", len(depsFile.Deps))
	}

	fooEntry := depsFile.Deps["github.com/foo/bar"]
	if fooEntry.Version != "v1.0.0" {
		t.Errorf("foo/bar Version = %q, expected 'v1.0.0'", fooEntry.Version)
	}
	if fooEntry.Indirect {
		t.Error("foo/bar should not be indirect")
	}

	bazEntry := depsFile.Deps["github.com/baz/qux"]
	if !bazEntry.Indirect {
		t.Error("baz/qux should be indirect")
	}
}
