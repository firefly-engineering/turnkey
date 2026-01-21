package starlark

import (
	"strings"
	"testing"
)

const sampleRulesStar = `# Sample rules.star file
load("@prelude//:rules.bzl", "go_library", "go_test")

# Main library
go_library(
    name = "mylib",
    srcs = ["foo.go", "bar.go"],
    deps = [
        "//pkg/foo:foo",
        "godeps//vendor/github.com/bar:bar",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = ["foo_test.go"],
    target_under_test = ":mylib",
    visibility = ["PUBLIC"],
)
`

func TestParse(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	// Check loads
	if len(f.Loads) != 1 {
		t.Errorf("Expected 1 load, got %d", len(f.Loads))
	}
	if f.Loads[0].Module != "@prelude//:rules.bzl" {
		t.Errorf("Expected module @prelude//:rules.bzl, got %s", f.Loads[0].Module)
	}
	if len(f.Loads[0].Symbols) != 2 {
		t.Errorf("Expected 2 symbols, got %d", len(f.Loads[0].Symbols))
	}

	// Check targets
	if len(f.Targets) != 2 {
		t.Errorf("Expected 2 targets, got %d", len(f.Targets))
	}

	// Check first target
	lib := f.GetTarget("mylib")
	if lib == nil {
		t.Fatal("Target mylib not found")
	}
	if lib.Rule != "go_library" {
		t.Errorf("Expected rule go_library, got %s", lib.Rule)
	}

	// Check srcs attribute
	srcs := lib.GetAttribute("srcs")
	if srcs == nil {
		t.Fatal("srcs attribute not found")
	}
	if list, ok := srcs.Value.(StringListValue); ok {
		if len(list.Values) != 2 {
			t.Errorf("Expected 2 srcs, got %d", len(list.Values))
		}
		if list.Values[0] != "foo.go" {
			t.Errorf("Expected foo.go, got %s", list.Values[0])
		}
	} else {
		t.Errorf("srcs is not a StringListValue: %T", srcs.Value)
	}

	// Check deps attribute
	deps := lib.GetDeps()
	if len(deps) != 2 {
		t.Errorf("Expected 2 deps, got %d", len(deps))
	}
	if deps[0] != "//pkg/foo:foo" {
		t.Errorf("Expected //pkg/foo:foo, got %s", deps[0])
	}

	// Check second target
	test := f.GetTarget("mylib_test")
	if test == nil {
		t.Fatal("Target mylib_test not found")
	}
	if test.Rule != "go_test" {
		t.Errorf("Expected rule go_test, got %s", test.Rule)
	}

	// Check target_under_test attribute
	tut := test.GetStringAttr("target_under_test")
	if tut != ":mylib" {
		t.Errorf("Expected :mylib, got %s", tut)
	}
}

func TestParseWithGlob(t *testing.T) {
	source := `load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = glob(["*.go"]),
    visibility = ["PUBLIC"],
)
`

	f, err := Parse("test.star", []byte(source))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	lib := f.GetTarget("mylib")
	if lib == nil {
		t.Fatal("Target not found")
	}

	srcs := lib.GetAttribute("srcs")
	if srcs == nil {
		t.Fatal("srcs attribute not found")
	}

	// glob() should be preserved as ExprValue
	if _, ok := srcs.Value.(ExprValue); !ok {
		t.Errorf("Expected ExprValue for glob, got %T", srcs.Value)
	}
}

func TestParseWithBooleans(t *testing.T) {
	source := `load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = ["main.go"],
    optimizer = True,
    debug = False,
)
`

	f, err := Parse("test.star", []byte(source))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	lib := f.GetTarget("mylib")
	if lib == nil {
		t.Fatal("Target not found")
	}

	optimizer := lib.GetAttribute("optimizer")
	if optimizer == nil {
		t.Fatal("optimizer attribute not found")
	}
	if b, ok := optimizer.Value.(BoolValue); !ok || !b.Value {
		t.Errorf("Expected True, got %v", optimizer.Value)
	}

	debug := lib.GetAttribute("debug")
	if debug == nil {
		t.Fatal("debug attribute not found")
	}
	if b, ok := debug.Value.(BoolValue); !ok || b.Value {
		t.Errorf("Expected False, got %v", debug.Value)
	}
}

func TestRoundTrip(t *testing.T) {
	// Parse and write back without modifications should produce identical output
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	output := f.Write()
	if string(output) != sampleRulesStar {
		t.Errorf("Round-trip failed.\nExpected:\n%s\nGot:\n%s", sampleRulesStar, string(output))
	}
}

func TestMutationTracking(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if f.IsModified() {
		t.Error("File should not be modified after parse")
	}

	lib := f.GetTarget("mylib")
	if lib.IsModified() {
		t.Error("Target should not be modified after parse")
	}

	// Make a modification
	lib.AddDep("//new:dep")

	if !lib.IsModified() {
		t.Error("Target should be modified after AddDep")
	}
	if !f.IsModified() {
		t.Error("File should be modified after target modification")
	}
}

func TestModifyDeps(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	lib := f.GetTarget("mylib")

	// Add a new dep
	lib.AddDep("//new:dep")
	deps := lib.GetDeps()
	if len(deps) != 3 {
		t.Errorf("Expected 3 deps, got %d", len(deps))
	}

	// Remove a dep
	lib.RemoveDep("//pkg/foo:foo")
	deps = lib.GetDeps()
	if len(deps) != 2 {
		t.Errorf("Expected 2 deps, got %d", len(deps))
	}

	// Adding existing dep should not duplicate
	lib.AddDep("//new:dep")
	deps = lib.GetDeps()
	if len(deps) != 2 {
		t.Errorf("Expected 2 deps after duplicate add, got %d", len(deps))
	}

	// Set deps completely
	lib.SetDeps([]string{"//only:one"})
	deps = lib.GetDeps()
	if len(deps) != 1 || deps[0] != "//only:one" {
		t.Errorf("SetDeps failed: %v", deps)
	}
}

func TestAddTarget(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	// Add new target
	newTarget := f.AddTarget("go_binary", "mybin")
	newTarget.SetString("main", "main.go")
	newTarget.SetDeps([]string{"//pkg:lib"})

	if len(f.Targets) != 3 {
		t.Errorf("Expected 3 targets, got %d", len(f.Targets))
	}

	found := f.GetTarget("mybin")
	if found == nil {
		t.Fatal("New target not found")
	}
	if found.Rule != "go_binary" {
		t.Errorf("Expected go_binary, got %s", found.Rule)
	}
}

func TestRemoveTarget(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	removed := f.RemoveTarget("mylib_test")
	if !removed {
		t.Error("RemoveTarget should return true")
	}
	if len(f.Targets) != 1 {
		t.Errorf("Expected 1 target, got %d", len(f.Targets))
	}
	if f.GetTarget("mylib_test") != nil {
		t.Error("Removed target should not be found")
	}

	// Removing non-existent target
	removed = f.RemoveTarget("nonexistent")
	if removed {
		t.Error("RemoveTarget should return false for non-existent target")
	}
}

func TestWriteModified(t *testing.T) {
	f, err := Parse("test.star", []byte(sampleRulesStar))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	lib := f.GetTarget("mylib")
	lib.SetDeps([]string{"//new:dep"})

	output := string(f.Write())

	// Output should contain the new dep
	if !strings.Contains(output, `"//new:dep"`) {
		t.Error("Output should contain new dep")
	}

	// Output should NOT contain the old deps
	if strings.Contains(output, `"//pkg/foo:foo"`) {
		t.Error("Output should not contain old dep")
	}

	// Output should still contain unchanged parts
	if !strings.Contains(output, "go_library") {
		t.Error("Output should contain go_library")
	}
	if !strings.Contains(output, "mylib_test") {
		t.Error("Output should contain unchanged mylib_test target")
	}
}
