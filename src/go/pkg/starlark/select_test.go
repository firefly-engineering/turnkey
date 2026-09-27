package starlark

import (
	"reflect"
	"strings"
	"testing"
)

const selectRulesStar = `load("@prelude//:rules.bzl", "rust_library")

rust_library(
    name = "lib",
    srcs = glob(["src/**/*.rs"]),
    deps = [
        # turnkey:auto-start
        "rustdeps//vendor/libc:libc",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//native:lib",  # hand-written
        # turnkey:preserve-end
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        # macOS mounts through FUSE-T
        "config//os:macos": [
            "rustdeps//vendor/fuse-t:fuse-t",
            "rustdeps//vendor/objc:objc",
        ],
    }),
    visibility = ["PUBLIC"],
)
`

func TestParseSelect(t *testing.T) {
	f, err := Parse("rules.star", []byte(selectRulesStar))
	if err != nil {
		t.Fatal(err)
	}
	sel, ok := f.GetTarget("lib").GetSelect("deps")
	if !ok {
		t.Fatalf("deps is %T, want a SelectValue", f.GetTarget("lib").GetAttribute("deps").Value)
	}
	common, ok := sel.Common.(DepsValue)
	if !ok || !reflect.DeepEqual(common.AutoDeps, []string{"rustdeps//vendor/libc:libc"}) ||
		!reflect.DeepEqual(common.PreservedDeps, []string{"//native:lib"}) {
		t.Errorf("common = %+v, want the marked list", sel.Common)
	}
	want := []SelectBranch{
		{Key: "config//os:linux", Value: StringListValue{Values: []string{"rustdeps//vendor/fuser:fuser"}}},
		{Key: "config//os:macos", Value: StringListValue{Values: []string{"rustdeps//vendor/fuse-t:fuse-t", "rustdeps//vendor/objc:objc"}}},
	}
	if !reflect.DeepEqual(sel.Branches, want) {
		t.Errorf("branches = %+v, want %+v", sel.Branches, want)
	}
	if got := f.GetTarget("lib").GetPreservedLabels("deps"); !reflect.DeepEqual(got, []string{"//native:lib"}) {
		t.Errorf("preserved = %v, want the common part's", got)
	}
}

func TestSelectRoundTripIsByteIdentical(t *testing.T) {
	f, err := Parse("rules.star", []byte(selectRulesStar))
	if err != nil {
		t.Fatal(err)
	}
	if got := string(f.Write()); got != selectRulesStar {
		t.Errorf("round trip changed the file:\n%s", got)
	}

	// Setting the same value is no change either
	lib := f.GetTarget("lib")
	sel, _ := lib.GetSelect("deps")
	lib.SetSelect("deps", []string{"rustdeps//vendor/libc:libc"}, sel.Branches)
	if f.IsModified() {
		t.Error("setting the same select() modified the target")
	}
	if got := string(f.Write()); got != selectRulesStar {
		t.Errorf("rewrite changed the file:\n%s", got)
	}
}

// Changing one branch rewrites only that branch: the other branch, its
// comment and the common part are untouched.
func TestSelectChangeRewritesOnlyThatBranch(t *testing.T) {
	f, err := Parse("rules.star", []byte(selectRulesStar))
	if err != nil {
		t.Fatal(err)
	}
	lib := f.GetTarget("lib")
	sel, _ := lib.GetSelect("deps")
	branches := append([]SelectBranch(nil), sel.Branches...)
	branches[0] = SelectBranch{Key: "config//os:linux", Value: StringListValue{Values: []string{
		"rustdeps//vendor/fuser:fuser",
		"rustdeps//vendor/nix:nix",
	}}}
	lib.SetSelect("deps", []string{"rustdeps//vendor/libc:libc"}, branches)

	want := strings.Replace(selectRulesStar,
		`"config//os:linux": ["rustdeps//vendor/fuser:fuser"],`,
		`"config//os:linux": [
            "rustdeps//vendor/fuser:fuser",
            "rustdeps//vendor/nix:nix",
        ],`, 1)
	if got := string(f.Write()); got != want {
		t.Errorf("output:\n%s\nwant:\n%s", got, want)
	}
}

// A plain list becomes the canonical [<common>] + select({...}), keeping
// its markers on the common part.
func TestSetSelectOnAPlainList(t *testing.T) {
	src := `rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
    ],
)
`
	f, err := Parse("rules.star", []byte(src))
	if err != nil {
		t.Fatal(err)
	}
	f.GetTarget("lib").SetSelect("deps", []string{"//a:a"}, []SelectBranch{
		{Key: "config//os:linux", Value: StringListValue{Values: []string{"//l:l"}}},
		{Key: "config//os:macos", Value: StringListValue{}},
	})
	want := `rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
    ] + select({
        "config//os:linux": ["//l:l"],
        "config//os:macos": [],
    }),
)
`
	got := string(f.Write())
	if got != want {
		t.Errorf("output:\n%s\nwant:\n%s", got, want)
	}

	// and back to a plain list
	f, err = Parse("rules.star", []byte(got))
	if err != nil {
		t.Fatal(err)
	}
	f.GetTarget("lib").SetSelect("deps", []string{"//a:a"}, nil)
	if got := string(f.Write()); got != src {
		t.Errorf("output:\n%s\nwant:\n%s", got, src)
	}
}

// Without common labels the value is the select() alone.
func TestSetSelectWithoutCommon(t *testing.T) {
	f, err := Parse("rules.star", []byte("rust_library(\n    name = \"lib\",\n    deps = [],\n)\n"))
	if err != nil {
		t.Fatal(err)
	}
	f.GetTarget("lib").SetSelect("deps", nil, []SelectBranch{
		{Key: "config//os:linux", Value: StringListValue{Values: []string{"//l:l"}}},
		{Key: "config//os:macos", Value: StringListValue{}},
	})
	want := "rust_library(\n    name = \"lib\",\n    deps = select({\n        \"config//os:linux\": [\"//l:l\"],\n        \"config//os:macos\": [],\n    }),\n)\n"
	if got := string(f.Write()); got != want {
		t.Errorf("output:\n%s\nwant:\n%s", got, want)
	}
}

// A list with markers holding something other than a label is not a label
// list: reading it would drop the nested select() on rewrite.
func TestMarkedListWithNonLabelIsUnreadable(t *testing.T) {
	src := `rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
        _EXTRA,
    ],
)
`
	f, err := Parse("rules.star", []byte(src))
	if err != nil {
		t.Fatal(err)
	}
	if v := f.GetTarget("lib").GetAttribute("deps").Value; v.Type() != TypeExpr {
		t.Errorf("deps parsed as %T, want an expression", v)
	}
}

// Other shapes around select() stay expressions.
func TestOtherSelectShapesAreExpressions(t *testing.T) {
	for _, value := range []string{
		`select({"config//os:linux": []}) + ["//a:a"]`,
		`_COMMON + select({"config//os:linux": []})`,
		`select({_KEY: []})`,
	} {
		f, err := Parse("rules.star", []byte("rust_library(\n    name = \"lib\",\n    deps = "+value+",\n)\n"))
		if err != nil {
			t.Fatal(err)
		}
		if v := f.GetTarget("lib").GetAttribute("deps").Value; v.Type() != TypeExpr {
			t.Errorf("deps = %s parsed as %T, want an expression", value, v)
		}
	}
}

// A new attribute goes on its own line before the closing parenthesis,
// leaving the rest of the target, comments included, as written.
func TestNewAttributeIsInserted(t *testing.T) {
	src := `rust_library(
    name = "lib",
    # the crate's sources
    srcs = glob(["src/**/*.rs"]),
)
`
	f, err := Parse("rules.star", []byte(src))
	if err != nil {
		t.Fatal(err)
	}
	f.GetTarget("lib").SetSelect("features", []string{"default"}, nil)
	want := `rust_library(
    name = "lib",
    # the crate's sources
    srcs = glob(["src/**/*.rs"]),
    features = ["default"],
)
`
	if got := string(f.Write()); got != want {
		t.Errorf("output:\n%s\nwant:\n%s", got, want)
	}
}
