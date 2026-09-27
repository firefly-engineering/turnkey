package mapper

import "testing"

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
	testRuleKinds(t, newGoLanguage(t.TempDir()), []ruleKindCase{
		{"go_library", true, Library},
		{"go_binary", true, Binary},
		{"go_test", true, Test},
		// contains "test" but isn't a test rule
		{"go_attested_library", false, NotSynced},
		{"rust_binary", false, NotSynced},
	})
}

func TestRustRuleKinds(t *testing.T) {
	testRuleKinds(t, newRustLanguage(t.TempDir()), []ruleKindCase{
		{"rust_library", true, Library},
		{"rust_binary", true, Binary},
		{"rust_test", true, Test},
		{"rust_testdata", false, NotSynced},
		{"go_test", false, NotSynced},
	})
}

func TestPythonRuleKinds(t *testing.T) {
	testRuleKinds(t, newPythonLanguage(t.TempDir()), []ruleKindCase{
		{"python_library", true, Library},
		{"python_binary", true, Binary},
		{"python_test", true, Test},
		{"python_test_utils", false, NotSynced},
		{"sh_binary", false, NotSynced},
	})
}

func TestTypeScriptRuleKinds(t *testing.T) {
	testRuleKinds(t, newTypeScriptLanguage(t.TempDir()), []ruleKindCase{
		{"typescript_library", true, Library},
		{"typescript_binary", true, Binary},
		{"js_library", true, Library},
		{"js_test", true, Test},
		{"js_contest_bundle", false, NotSynced},
	})
}

func TestSolidityRuleKinds(t *testing.T) {
	testRuleKinds(t, newSolidityLanguage(t.TempDir()), []ruleKindCase{
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
	m, err := New(Config{ProjectRoot: t.TempDir()})
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
