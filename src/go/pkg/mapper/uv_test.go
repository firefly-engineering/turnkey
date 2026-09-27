package mapper

import (
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

// uvWorkspaceFixture is a uv workspace whose members share the acme.*
// namespace: libs/cfg in a flat layout, apps/app in a src/ layout, and two
// members that both provide a top-level "tools" package.
func uvWorkspaceFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"pyproject.toml": `[project]
name = "acme"

[tool.uv.workspace]
members = ["libs/*", "apps/app"]
`,
		"python-deps.toml": `[deps.requests]
version = "2.0"
`,
		"libs/cfg/pyproject.toml":              "[project]\nname = \"acme-cfg\"\n",
		"libs/cfg/acme/cfg/__init__.py":        "",
		"libs/cfg/acme/cfg/parser.py":          "",
		"libs/cfg/tests/test_parser.py":        "",
		"libs/cfg/tools/__init__.py":           "",
		"libs/other/pyproject.toml":            "[project]\nname = \"acme-other\"\n",
		"libs/other/tools/__init__.py":         "",
		"apps/app/pyproject.toml":              "[project]\nname = \"acme-app\"\n",
		"apps/app/src/acme/app/__init__.py":    "",
		"apps/app/src/acme/app/main.py":        "",
		"libs/not-a-member/acme/x/__init__.py": "",
	})
	return root
}

func TestLoadUVWorkspaceModules(t *testing.T) {
	root := uvWorkspaceFixture(t)
	modules, err := loadUVWorkspaceModules(root)
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]string{
		"acme.cfg": "libs/cfg",
		"acme.app": "apps/app",
		"tests":    "libs/cfg",
		// "tools" is provided by two members: ambiguous, left out
	}
	if !reflect.DeepEqual(modules, want) {
		t.Errorf("modules = %v, want %v", modules, want)
	}
}

func TestMapPythonWorkspaceImports(t *testing.T) {
	root := uvWorkspaceFixture(t)
	m, err := New(Config{ProjectRoot: root})
	if err != nil {
		t.Fatal(err)
	}

	result := extraction.NewResult("python")
	result.AddPackage(extraction.Package{
		Path: "acme/app",
		Imports: []extraction.Import{
			// dotted import of another member's module
			{Path: "acme.cfg.parser", Kind: extraction.ImportKindExternal},
			// from acme import cfg, as the extractor reports it
			{Path: "acme.cfg", Kind: extraction.ImportKindExternal},
			// the member's own module: maps to its own target, which sync
			// filters out, rather than being reported unmapped
			{Path: "acme.app.main", Kind: extraction.ImportKindExternal},
			// third party, unchanged
			{Path: "requests.adapters", Kind: extraction.ImportKindExternal},
			// ambiguous between two members
			{Path: "tools", Kind: extraction.ImportKindExternal},
		},
	})
	mappings, err := m.MapExtractionResult(result)
	if err != nil {
		t.Fatal(err)
	}
	mapping := mappings["acme/app"]

	wantDeps := []string{
		"//apps/app:app",
		"//libs/cfg:cfg",
		"pydeps//vendor/requests:requests",
	}
	if got := targets(mapping.Deps); !reflect.DeepEqual(got, wantDeps) {
		t.Errorf("Deps = %v, want %v", got, wantDeps)
	}
	if got, want := mapping.UnmappedImports, []string{"tools"}; !reflect.DeepEqual(got, want) {
		t.Errorf("UnmappedImports = %v, want %v", got, want)
	}
}
