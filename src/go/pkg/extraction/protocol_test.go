package extraction

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

func TestNewResult(t *testing.T) {
	r := NewResult("go")

	if r.Version != ProtocolVersion {
		t.Errorf("expected version %s, got %s", ProtocolVersion, r.Version)
	}
	if r.Language != "go" {
		t.Errorf("expected language go, got %s", r.Language)
	}
	if len(r.Packages) != 0 {
		t.Errorf("expected empty packages, got %d", len(r.Packages))
	}
}

func TestAddPackage(t *testing.T) {
	r := NewResult("go")

	r.AddPackage(Package{
		Path:  "src/cmd/tk",
		Files: []string{"main.go"},
		Imports: []Import{
			{Path: "fmt", Kind: ImportKindStdlib},
			{Path: "github.com/foo/bar", Kind: ImportKindExternal},
		},
	})

	if len(r.Packages) != 1 {
		t.Fatalf("expected 1 package, got %d", len(r.Packages))
	}

	pkg := r.Packages[0]
	if pkg.Path != "src/cmd/tk" {
		t.Errorf("expected path src/cmd/tk, got %s", pkg.Path)
	}
	if len(pkg.Imports) != 2 {
		t.Errorf("expected 2 imports, got %d", len(pkg.Imports))
	}
}

func TestAddError(t *testing.T) {
	r := NewResult("go")
	r.AddError("failed to parse foo.go")
	r.AddError("unknown import in bar.go")

	if len(r.Errors) != 2 {
		t.Errorf("expected 2 errors, got %d", len(r.Errors))
	}
}

func TestWriteAndParse(t *testing.T) {
	// Create a result
	r := NewResult("go")
	r.AddPackage(Package{
		Path:  "src/cmd/tk",
		Files: []string{"main.go", "sync.go"},
		Imports: []Import{
			{Path: "fmt", Kind: ImportKindStdlib},
			{Path: "github.com/foo/bar", Kind: ImportKindExternal},
			{Path: "github.com/firefly-engineering/turnkey/src/go/pkg/rules", Kind: ImportKindInternal},
		},
		TestImports: []Import{
			{Path: "testing", Kind: ImportKindStdlib},
		},
		BuildTags: []string{"linux"},
	})
	r.AddError("warning: skipped generated file")

	// Write to buffer
	var buf bytes.Buffer
	if err := r.Write(&buf); err != nil {
		t.Fatalf("Write failed: %v", err)
	}

	// Parse back
	parsed, err := Parse(&buf)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	// Verify
	if parsed.Version != r.Version {
		t.Errorf("version mismatch: %s != %s", parsed.Version, r.Version)
	}
	if parsed.Language != r.Language {
		t.Errorf("language mismatch: %s != %s", parsed.Language, r.Language)
	}
	if len(parsed.Packages) != len(r.Packages) {
		t.Errorf("packages count mismatch: %d != %d", len(parsed.Packages), len(r.Packages))
	}
	if len(parsed.Errors) != len(r.Errors) {
		t.Errorf("errors count mismatch: %d != %d", len(parsed.Errors), len(r.Errors))
	}

	pkg := parsed.Packages[0]
	if len(pkg.Imports) != 3 {
		t.Errorf("expected 3 imports, got %d", len(pkg.Imports))
	}
	if len(pkg.TestImports) != 1 {
		t.Errorf("expected 1 test import, got %d", len(pkg.TestImports))
	}
	if len(pkg.BuildTags) != 1 {
		t.Errorf("expected 1 build tag, got %d", len(pkg.BuildTags))
	}
}

func TestParseUnsupportedVersion(t *testing.T) {
	data := `{"version": "999", "language": "go", "packages": []}`
	_, err := Parse(strings.NewReader(data))
	if err == nil {
		t.Error("expected error for unsupported version")
	}
	if !strings.Contains(err.Error(), "unsupported protocol version") {
		t.Errorf("unexpected error message: %v", err)
	}
}

func TestJSONFormat(t *testing.T) {
	r := NewResult("rust")
	r.AddPackage(Package{
		Path:  "src/mylib",
		Files: []string{"lib.rs"},
		Imports: []Import{
			{Path: "serde", Kind: ImportKindExternal, Alias: ""},
			{Path: "crate::util", Kind: ImportKindInternal},
		},
	})

	var buf bytes.Buffer
	if err := r.Write(&buf); err != nil {
		t.Fatalf("Write failed: %v", err)
	}

	// Verify it's valid JSON
	var raw map[string]interface{}
	if err := json.Unmarshal(buf.Bytes(), &raw); err != nil {
		t.Fatalf("Invalid JSON: %v", err)
	}

	// Check structure
	if raw["version"] != "1" {
		t.Errorf("expected version 1, got %v", raw["version"])
	}
	if raw["language"] != "rust" {
		t.Errorf("expected language rust, got %v", raw["language"])
	}
}

func TestImportKindConstants(t *testing.T) {
	// Verify the string values are what we expect
	tests := []struct {
		kind     ImportKind
		expected string
	}{
		{ImportKindInternal, "internal"},
		{ImportKindExternal, "external"},
		{ImportKindStdlib, "stdlib"},
	}

	for _, tt := range tests {
		if string(tt.kind) != tt.expected {
			t.Errorf("ImportKind %v has wrong string value: got %s, want %s", tt.kind, string(tt.kind), tt.expected)
		}
	}
}
