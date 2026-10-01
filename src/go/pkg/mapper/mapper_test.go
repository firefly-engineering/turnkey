package mapper

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
	"github.com/firefly-engineering/turnkey/src/go/pkg/godeps"
)

func TestMapGoImports(t *testing.T) {
	lang := &goLanguage{
		cfg: &GoConfig{
			Members:      []GoMember{{Path: "github.com/firefly-engineering/turnkey", Dir: "."}},
			ExternalCell: "godeps",
			ExternalDeps: map[string]bool{
				"github.com/google/uuid": true,
				"golang.org/x/sys":       true,
				"go.starlark.net":        true,
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
			deps, _ := mapImports(lang, tt.imports)
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

// An extraction result's imports are mapped with the language: stdlib
// skipped, internal and external imports mapped.
func TestResolveImports(t *testing.T) {
	lang := &goLanguage{cfg: &GoConfig{
		Members:      []GoMember{{Path: "github.com/example/project", Dir: "."}},
		ExternalCell: "godeps",
		ExternalDeps: map[string]bool{
			"github.com/google/uuid": true,
		},
	}}

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

	mapping := resolveImports(lang, result)
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

// The mapper reads the go-deps.toml godeps-gen writes: its keys carry a
// version ("path@version"), so imports must match on import_path.
func TestGoDepsFromGodepsGen(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte("module github.com/example/project\n"), 0644); err != nil {
		t.Fatal(err)
	}
	depsFile, err := os.Create(filepath.Join(dir, "go-deps.toml"))
	if err != nil {
		t.Fatal(err)
	}
	deps := []godeps.Dependency{
		{ImportPath: "github.com/pelletier/go-toml/v2", Version: "v2.2.4"},
		{ImportPath: "golang.org/x/mod", Version: "v0.31.0"},
	}
	if err := godeps.WriteTOML(depsFile, deps, godeps.DefaultOutputOptions()); err != nil {
		t.Fatal(err)
	}
	if err := depsFile.Close(); err != nil {
		t.Fatal(err)
	}

	m, err := New(testConfig(dir))
	if err != nil {
		t.Fatal(err)
	}

	mapped, unmapped := mapImports(m.Language("go").(importLanguage), []extraction.Import{
		{Path: "github.com/pelletier/go-toml/v2", Kind: extraction.ImportKindExternal},
		{Path: "golang.org/x/mod/modfile", Kind: extraction.ImportKindExternal},
	})

	if len(unmapped) != 0 {
		t.Errorf("unmapped = %v, want none", unmapped)
	}
	want := []string{
		"godeps//vendor/github.com/pelletier/go-toml/v2:v2",
		"godeps//vendor/golang.org/x/mod/modfile:modfile",
	}
	if got := DepsToTargets(mapped); !reflect.DeepEqual(got, want) {
		t.Errorf("targets = %v, want %v", got, want)
	}
}

func TestUnmappedExternalDep(t *testing.T) {
	lang := &goLanguage{
		cfg: &GoConfig{
			Members:      []GoMember{{Path: "github.com/example/project", Dir: "."}},
			ExternalCell: "godeps",
			ExternalDeps: map[string]bool{
				// Only uuid is known
				"github.com/google/uuid": true,
			},
		},
	}

	imports := []extraction.Import{
		{Path: "github.com/unknown/package", Kind: extraction.ImportKindExternal},
	}

	deps, unmapped := mapImports(lang, imports)

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

// An import maps to the workspace member whose module path is its longest
// prefix on a / boundary, at its directory, with the target named after the
// import path's last component (as the godeps cell's forwarding aliases
// name it), and the longest module path wins between a member and a
// third-party module.
func TestMapGoWorkspaceImports(t *testing.T) {
	cfg := &GoConfig{
		Members: []GoMember{
			{Path: "example.com/app", Dir: "."},
			{Path: "example.com/app/tools", Dir: "tools"},
			{Path: "example.com/lib", Dir: "libs/lib"},
			{Path: "golang.org/x/sys", Dir: "third_party/x-sys"},
		},
		ExternalCell: "godeps",
		ExternalDeps: map[string]bool{
			"example.com/libextra":  true,
			"example.com/lib/proto": true,
		},
	}
	lang := &goLanguage{cfg: cfg}

	tests := []struct {
		imp  string
		kind extraction.ImportKind
		want string
	}{
		{"example.com/app/internal/x", extraction.ImportKindInternal, "//internal/x:x"},
		{"example.com/app", extraction.ImportKindInternal, "//:app"},
		{"example.com/app/tools/gen", extraction.ImportKindInternal, "//tools/gen:gen"},
		{"example.com/lib/sub", extraction.ImportKindInternal, "//libs/lib/sub:sub"},
		{"golang.org/x/sys", extraction.ImportKindInternal, "//third_party/x-sys:sys"},
		{"golang.org/x/sys/cpu", extraction.ImportKindInternal, "//third_party/x-sys/cpu:cpu"},
		// Not on a / boundary: a third-party module
		{"example.com/libextra/y", extraction.ImportKindExternal, "godeps//vendor/example.com/libextra/y:y"},
		// A third-party module nested in a member's module path
		{"example.com/lib/proto/v1", extraction.ImportKindExternal, "godeps//vendor/example.com/lib/proto/v1:v1"},
		{"fmt", extraction.ImportKindStdlib, ""},
	}
	for _, tt := range tests {
		if got := classifyGoImport(tt.imp, cfg); got != tt.kind {
			t.Errorf("classifyGoImport(%q) = %v, want %v", tt.imp, got, tt.kind)
			continue
		}
		deps, unmapped := mapImports(lang, []extraction.Import{{Path: tt.imp, Kind: tt.kind}})
		if len(unmapped) != 0 {
			t.Errorf("%s: unmapped", tt.imp)
		}
		var got string
		if targets := DepsToTargets(deps); len(targets) == 1 {
			got = targets[0]
		}
		if got != tt.want {
			t.Errorf("%s maps to %q, want %q", tt.imp, got, tt.want)
		}
	}
}

// An import is first-party only on a / boundary of a member's module path.
func TestClassifyGoImportBoundary(t *testing.T) {
	cfg := &GoConfig{Members: []GoMember{{Path: "github.com/x/fo", Dir: "."}}}
	if got := classifyGoImport("github.com/x/foo", cfg); got != extraction.ImportKindExternal {
		t.Errorf("github.com/x/foo in module github.com/x/fo: %v, want external", got)
	}
}

// The members come from go-deps.toml's [members] when it records them, and
// otherwise are the root go.mod alone.
func TestGoMembersFromDepsFile(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte("module example.com/root\n"), 0644); err != nil {
		t.Fatal(err)
	}
	lang := testLanguage("go")

	cfg, _ := detectGoConfig(dir, lang)
	if want := []GoMember{{Path: "example.com/root", Dir: "."}}; !reflect.DeepEqual(cfg.Members, want) {
		t.Errorf("without go-deps.toml: members = %v, want %v", cfg.Members, want)
	}

	depsFile, err := os.Create(filepath.Join(dir, lang.DepsFile))
	if err != nil {
		t.Fatal(err)
	}
	err = godeps.WriteDepsFile(depsFile, godeps.DepsFile{
		Members: []godeps.Member{{Path: "example.com/b", Dir: "b"}, {Path: "example.com/a", Dir: "a"}},
	}, godeps.DefaultOutputOptions())
	if err != nil {
		t.Fatal(err)
	}
	if err := depsFile.Close(); err != nil {
		t.Fatal(err)
	}

	cfg, _ = detectGoConfig(dir, lang)
	want := []GoMember{{Path: "example.com/a", Dir: "a"}, {Path: "example.com/b", Dir: "b"}}
	if !reflect.DeepEqual(cfg.Members, want) {
		t.Errorf("members = %v, want %v", cfg.Members, want)
	}
}

// Sync manages a package's Go rules only when the nearest go.mod above it
// is a member's.
func TestGoManages(t *testing.T) {
	root := t.TempDir()
	for _, f := range []string{"go.mod", "lib/go.mod", "src/testdata/fixture/go.mod"} {
		p := filepath.Join(root, f)
		if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte("module example.com/m\n"), 0644); err != nil {
			t.Fatal(err)
		}
	}
	lang := &goLanguage{projectRoot: root, cfg: &GoConfig{Members: []GoMember{
		{Path: "example.com/app", Dir: "."},
		{Path: "example.com/lib", Dir: "lib"},
	}}}

	for dir, want := range map[string]bool{
		".":                         true,
		"cmd/app":                   true,
		"lib":                       true,
		"lib/sub":                   true,
		"src/testdata/fixture":      false,
		"src/testdata/fixture/deep": false,
	} {
		if got := lang.Manages(filepath.Join(root, dir)); got != want {
			t.Errorf("Manages(%s) = %v, want %v", dir, got, want)
		}
	}

	// Without a member at the root, a package under no member isn't managed
	lang.cfg.Members = lang.cfg.Members[1:]
	if lang.Manages(filepath.Join(root, "cmd/app")) {
		t.Error("Manages(cmd/app) with no root member = true, want false")
	}
}
