package mapper

import (
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// testLanguages are a project's languages as sync.toml lists them, each
// with its default cell and deps file.
var testLanguages = []syncconfig.Language{
	{Name: "go", Cell: "godeps", DepsFile: "go-deps.toml"},
	{Name: "rust", Cell: "rustdeps", DepsFile: "rust-deps.toml"},
	{Name: "python", Cell: "pydeps", DepsFile: "python-deps.toml"},
	{Name: "javascript", Cell: "jsdeps", DepsFile: "js-deps.toml"},
	{Name: "solidity", Cell: "soldeps", DepsFile: "solidity-deps.toml"},
}

// testConfig is the configuration of a mapper for the project at root,
// with testLanguages.
func testConfig(root string) Config {
	return Config{ProjectRoot: root, Languages: testLanguages}
}

// testLanguage returns the language of testLanguages named name.
func testLanguage(name string) syncconfig.Language {
	for _, lang := range testLanguages {
		if lang.Name == name {
			return lang
		}
	}
	panic("no test language " + name)
}

// Each language sync.toml lists gets its plug-in, in the listed order.
func TestNewLanguages(t *testing.T) {
	m, err := New(testConfig(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, lang := range m.Languages() {
		names = append(names, lang.Name())
	}
	if got, want := strings.Join(names, " "), "go rust python typescript solidity"; got != want {
		t.Errorf("plug-ins = %s, want %s", got, want)
	}
}

// A language no plug-in serves is an error, rather than a language whose
// targets sync silently leaves alone.
func TestNewUnknownLanguage(t *testing.T) {
	_, err := New(Config{
		ProjectRoot: t.TempDir(),
		Languages:   []syncconfig.Language{{Name: "cobol", Cell: "cobdeps", DepsFile: "cobol-deps.toml"}},
	})
	if err == nil {
		t.Fatal("New with a language without a plug-in succeeded")
	}
}

// ruleKindCase is one rule kind and how a plug-in must classify it.
type ruleKindCase struct {
	rule  string
	owned bool
	kind  TargetKind
}

func testRuleKinds(t *testing.T, lang Language, cases []ruleKindCase) {
	t.Helper()
	for _, tc := range cases {
		kind, owned := lang.RuleKind(tc.rule)
		if owned != tc.owned || (owned && kind != tc.kind) {
			t.Errorf("%s.RuleKind(%q) = (%v, %v), want (%v, %v)",
				lang.Name(), tc.rule, kind, owned, tc.kind, tc.owned)
		}
	}
}

func TestGoRuleKinds(t *testing.T) {
	testRuleKinds(t, newGoLanguage(testConfig(t.TempDir()), testLanguage("go")), []ruleKindCase{
		{"go_library", true, Library},
		{"go_binary", true, Binary},
		{"go_test", true, Test},
		// contains "test" but isn't a test rule
		{"go_attested_library", false, NotSynced},
		{"rust_binary", false, NotSynced},
	})
}

func TestRustRuleKinds(t *testing.T) {
	testRuleKinds(t, newRustLanguage(testConfig(t.TempDir()), testLanguage("rust")), []ruleKindCase{
		{"rust_library", true, Library},
		{"rust_binary", true, Binary},
		{"rust_test", true, Test},
		{"rust_testdata", false, NotSynced},
		{"go_test", false, NotSynced},
	})
}

func TestPythonRuleKinds(t *testing.T) {
	testRuleKinds(t, newPythonLanguage(testConfig(t.TempDir()), testLanguage("python")), []ruleKindCase{
		{"python_library", true, Library},
		{"python_binary", true, Binary},
		{"python_test", true, Test},
		{"python_test_utils", false, NotSynced},
		{"sh_binary", false, NotSynced},
	})
}

func TestTypeScriptRuleKinds(t *testing.T) {
	testRuleKinds(t, newTypeScriptLanguage(testConfig(t.TempDir()), testLanguage("javascript")), []ruleKindCase{
		{"typescript_library", true, Library},
		{"typescript_binary", true, Binary},
		{"js_library", true, Library},
		{"js_test", true, Test},
		{"js_contest_bundle", false, NotSynced},
	})
}

func TestSolidityRuleKinds(t *testing.T) {
	testRuleKinds(t, newSolidityLanguage(testConfig(t.TempDir()), testLanguage("solidity")), []ruleKindCase{
		{"solidity_library", true, Library},
		{"solidity_test", true, Test},
		// a Solidity rule whose deps sync leaves alone
		{"solidity_contract", true, NotSynced},
		{"solidity_attestation", false, NotSynced},
	})
}

// Every binary rule of a supported language is synced like a library, and
// a rule no plug-in owns is left alone.
func TestRuleLanguage(t *testing.T) {
	m, err := New(testConfig(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	for rule, want := range map[string]string{
		"go_binary":     "go",
		"rust_binary":   "rust",
		"python_binary": "python",
		"sh_binary":     "",
		"genrule":       "",
	} {
		lang, kind := m.RuleLanguage(rule)
		got := ""
		if lang != nil {
			got = lang.Name()
		}
		if got != want {
			t.Errorf("RuleLanguage(%q) = %q, want %q", rule, got, want)
		}
		if want != "" && kind != Binary {
			t.Errorf("RuleLanguage(%q) kind = %v, want Binary", rule, kind)
		}
	}
}
