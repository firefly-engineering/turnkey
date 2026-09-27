package mapper

import (
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

// jsDepsFixture is a project whose js-deps.toml is in jsdeps-gen's format.
func jsDepsFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	writeTree(t, root, map[string]string{
		"package.json": "{}\n",
		"js-deps.toml": `[meta]
generator = "jsdeps-gen 0.1.0"

[[package]]
name = "lodash"
version = "4.17.21"
url = "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
integrity = "sha512-x"

[[package]]
name = "@types/lodash"
version = "4.17.23"
url = "https://registry.npmjs.org/@types%2flodash/-/lodash-4.17.23.tgz"
integrity = "sha512-y"

[[package]]
name = "@openzeppelin/contracts"
version = "5.4.0"
url = "https://registry.npmjs.org/@openzeppelin%2fcontracts/-/contracts-5.4.0.tgz"
integrity = "sha512-z"
`,
	})
	return root
}

func TestMapTypeScriptImports(t *testing.T) {
	m, err := New(testConfig(jsDepsFixture(t)))
	if err != nil {
		t.Fatal(err)
	}
	lang := m.Language("typescript").(*typescriptLanguage)
	if r, _ := lang.Rule("typescript_library"); r.DepsAttribute != "npm_deps" {
		t.Errorf("DepsAttribute = %q, want npm_deps", r.DepsAttribute)
	}

	result := extraction.NewResult("typescript")
	result.AddPackage(extraction.Package{
		Path: "app",
		Imports: []extraction.Import{
			// unscoped, with a subpath
			{Path: "lodash/fp", Kind: extraction.ImportKindExternal},
			// scoped
			{Path: "@openzeppelin/contracts/token", Kind: extraction.ImportKindExternal},
			// relative: same target
			{Path: "./util", Kind: extraction.ImportKindInternal},
			{Path: "left-pad", Kind: extraction.ImportKindExternal},
		},
	})
	mapping := resolveImports(lang, result)
	mapping.Deps = lang.withTypes(mapping.Deps)

	want := []string{
		"jsdeps//:lodash",
		"jsdeps//:openzeppelin_contracts",
		// lodash's DefinitelyTyped package, which code never imports
		"jsdeps//:types_lodash",
	}
	if got := targets(mapping.Deps); !reflect.DeepEqual(got, want) {
		t.Errorf("Deps = %v, want %v", got, want)
	}
	if got, want := mapping.UnmappedImports, []string{"left-pad"}; !reflect.DeepEqual(got, want) {
		t.Errorf("UnmappedImports = %v, want %v", got, want)
	}
}

func TestTypesPackage(t *testing.T) {
	for pkg, want := range map[string]string{
		"lodash":                  "@types/lodash",
		"@openzeppelin/contracts": "@types/openzeppelin__contracts",
		"@types/node":             "@types/node",
	} {
		if got := typesPackage(pkg); got != want {
			t.Errorf("typesPackage(%q) = %q, want %q", pkg, got, want)
		}
	}
}
