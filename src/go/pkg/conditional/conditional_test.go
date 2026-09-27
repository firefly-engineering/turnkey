package conditional

import (
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// turnkey's default platforms
var platforms = []conditions.Platform{
	{OS: "linux", CPU: "x86_64"},
	{OS: "linux", CPU: "arm64"},
	{OS: "macos", CPU: "x86_64"},
	{OS: "macos", CPU: "arm64"},
}

var (
	linuxX86 = conditions.Configuration{conditions.OS: "linux", conditions.CPU: "x86_64"}
	linuxArm = conditions.Configuration{conditions.OS: "linux", conditions.CPU: "arm64"}
	macosArm = conditions.Configuration{conditions.OS: "macos", conditions.CPU: "arm64"}
)

// target parses a rule with one attribute, deps, written as value.
func target(t *testing.T, value string) *starlark.Target {
	t.Helper()
	return file(t, value).Targets[0]
}

// file parses a rules.star of one rule with one attribute, deps, written
// as value.
func file(t *testing.T, value string) *starlark.File {
	t.Helper()
	src := "rust_library(\n    name = \"lib\",\n"
	if value != "" {
		src += "    deps = " + value + ",\n"
	}
	f, err := starlark.Parse("rules.star", []byte(src+")\n"))
	if err != nil {
		t.Fatal(err)
	}
	return f
}

// labelsCases are values per configuration to write and read back.
var labelsCases = map[string]func(conditions.Configuration) []string{
	"none":      func(conditions.Configuration) []string { return nil },
	"identical": func(conditions.Configuration) []string { return []string{"//a:a", "//b:b"} },
	"by os": func(c conditions.Configuration) []string {
		if c[conditions.OS] == "linux" {
			return []string{"//c:c", "//l:l"}
		}
		return []string{"//c:c"}
	},
	"only some": func(c conditions.Configuration) []string {
		if c.String() == linuxArm.String() {
			return []string{"//la:la"}
		}
		return nil
	},
	"by platform": func(c conditions.Configuration) []string {
		return []string{"//c:c", "//" + c[conditions.OS] + ":" + c[conditions.CPU]}
	},
}

// What LabelsValue writes reads back as every configuration's labels.
func TestLabelsValueRoundTrip(t *testing.T) {
	space := conditions.NewSpace(platforms, "")
	for name, labels := range labelsCases {
		value := LabelsValue(space, labels)
		written := ""
		if value != nil {
			written = starlark.RenderIndented(value, "    ")
		}
		read, err := ReadLabels(target(t, written), "deps", space)
		if err != nil {
			t.Fatalf("%s: reading back %s: %v", name, written, err)
		}
		for _, config := range space.Configurations {
			if got, want := read(config), labels(config); !sameSet(got, want) {
				t.Errorf("%s, %s: read back %v, want %v", name, config, got, want)
			}
		}
	}
}

// What SetLabels writes reads back as every configuration's labels, over
// a space with a Go build tag too.
func TestSetLabelsRoundTrip(t *testing.T) {
	space := conditions.NewSpace(platforms, "").WithDimensions([]string{conditions.GoTag("integration")})
	cases := map[string]func(conditions.Configuration) []string{
		"by tag and os": func(c conditions.Configuration) []string {
			if c[conditions.OS] == "linux" && c[conditions.GoTag("integration")] == conditions.Set {
				return []string{"//it:it"}
			}
			return nil
		},
	}
	for name, labels := range labelsCases {
		cases[name] = labels
	}
	for name, labels := range cases {
		f := file(t, `["//old:old"]`)
		SetLabels(f.Targets[0], "deps", space, labels)
		read, err := ReadLabels(reparse(t, f), "deps", space)
		if err != nil {
			t.Fatalf("%s: reading back: %v", name, err)
		}
		for _, config := range space.Configurations {
			if got, want := read(config), labels(config); !sameSet(got, want) {
				t.Errorf("%s, %s: read back %v, want %v", name, config, got, want)
			}
		}
	}
}

// SetLabels writes the plain part into the auto-managed section, keeping
// the preserved one.
func TestSetLabelsKeepsMarkers(t *testing.T) {
	space := conditions.NewSpace(platforms, "")
	f := file(t, "[\n        # turnkey:auto-start\n        \"//old:old\",\n        # turnkey:auto-end\n        # turnkey:preserve-start\n        \"//p:p\",\n        # turnkey:preserve-end\n    ]")
	SetLabels(f.Targets[0], "deps", space, labelsCases["by os"])
	got := string(f.Write())
	for _, want := range []string{"turnkey:auto-start", "\"//c:c\"", "turnkey:preserve-start", "\"//p:p\"", "select("} {
		if !strings.Contains(got, want) {
			t.Errorf("written rules.star lacks %s:\n%s", want, got)
		}
	}
	if strings.Contains(got, "//old:old") {
		t.Errorf("written rules.star still has //old:old:\n%s", got)
	}
}

// No configuration having a label is no value, for cell generators to
// leave the attribute out.
func TestLabelsValueOfNothingIsNil(t *testing.T) {
	if v := LabelsValue(conditions.NewSpace(platforms, ""), labelsCases["none"]); v != nil {
		t.Errorf("value = %s, want nil", starlark.Render(v))
	}
}

// A label list given as an expression, or as a select() whose keys the
// space doesn't know, isn't read: writing back would replace it.
func TestReadLabelsUnreadable(t *testing.T) {
	space := conditions.NewSpace(platforms, "")
	for _, tc := range []struct {
		deps string
		want bool
	}{
		{"", true},
		{`["//a:a"]`, true},
		{"[\n        # turnkey:auto-start\n        \"//a:a\",\n        # turnkey:auto-end\n    ]", true},
		{`_DEPS`, false},
		{`_DEPS + ["//a:a"]`, false},
		{`["//a:a"] if X else []`, false},
		{`"//a:a"`, false},
		{`["//a:a"] + select({"config//os:linux": ["//l:l"], "config//os:macos": []})`, true},
		{`select({"config//os:linux": ["//l:l"], "DEFAULT": []})`, true},
		{`select({"//my:setting": ["//l:l"]})`, false},
		{`select({"config//os:linux": _LINUX})`, false},
		{`select({"config//os:linux": True})`, false},
		{`["//a:a"] + select({"config//os:linux": _LINUX})`, false},
	} {
		_, err := ReadLabels(target(t, tc.deps), "deps", space)
		if got := err == nil; got != tc.want {
			t.Errorf("ReadLabels(deps = %s) readable = %v, want %v (%v)", tc.deps, got, tc.want, err)
		}
	}
}

// Read takes any value that isn't a select() as is, and a select() alone
// of other values than lists; it refuses unknown keys, and a plain part
// followed by anything but a list.
func TestReadUnreadable(t *testing.T) {
	space := conditions.NewSpace(platforms, "")
	for _, tc := range []struct {
		value string
		want  bool
	}{
		{`_DEPS`, true},
		{`True`, true},
		{`select({"config//os:linux": False, "DEFAULT": True})`, true},
		{`select({"config//os:linux": _LINUX})`, true},
		{`select({"//my:setting": True})`, false},
		{`["//a:a"] + select({"config//os:linux": _LINUX})`, false},
		{`["//a:a"] + select({"config//os:linux": "//l:l"})`, false},
	} {
		_, err := Read(target(t, tc.value), "deps", space)
		if got := err == nil; got != tc.want {
			t.Errorf("Read(%s) readable = %v, want %v (%v)", tc.value, got, tc.want, err)
		}
	}
}

// The policy for each configuration: the branch that applies is the most
// specific matching key, else DEFAULT; the plain part comes first and each
// label is kept once; with no branch applying, the plain part alone.
func TestReadPolicy(t *testing.T) {
	space := conditions.NewSpace(platforms, "")
	for _, tc := range []struct {
		value  string
		config conditions.Configuration
		want   starlark.AttributeValue
	}{
		// absent
		{"", linuxX86, nil},
		// not a select()
		{`True`, linuxX86, starlark.BoolValue{Value: true}},
		{`["//b:b", "//b:b"]`, linuxX86, starlark.StringListValue{Values: []string{"//b:b", "//b:b"}}},
		// DEFAULT, and the most specific key
		{`select({"config//os:linux": ["//l:l"], "DEFAULT": ["//d:d"]})`, macosArm, list("//d:d")},
		{`select({"config//os:linux": ["//l:l"], "DEFAULT": ["//d:d"]})`, linuxArm, list("//l:l")},
		{`select({"config//os:linux": ["//l:l"], "toolchains//conditions:linux-arm64": ["//la:la"]})`, linuxArm, list("//la:la")},
		// the plain part first, each label once
		{`["//c:c", "//l:l"] + select({"config//os:linux": ["//l:l", "//x:x"]})`, linuxX86, list("//c:c", "//l:l", "//x:x")},
		{`select({"config//os:linux": ["//l:l", "//l:l"]})`, linuxX86, list("//l:l")},
		// no branch applies: the plain part alone
		{`["//c:c"] + select({"config//os:linux": ["//l:l"]})`, macosArm, list("//c:c")},
		{`select({"config//os:linux": ["//l:l"]})`, macosArm, nil},
		{`select({"config//os:linux": False})`, macosArm, nil},
		// a select() alone of other values
		{`select({"config//os:linux": False, "DEFAULT": True})`, linuxX86, starlark.BoolValue{Value: false}},
	} {
		read, err := Read(target(t, tc.value), "deps", space)
		if err != nil {
			t.Fatalf("Read(%s): %v", tc.value, err)
		}
		if got := read(tc.config); !reflect.DeepEqual(got, tc.want) {
			t.Errorf("Read(%s) in %s = %#v, want %#v", tc.value, tc.config, got, tc.want)
		}
	}
}

// list returns a StringListValue of labels.
func list(labels ...string) starlark.AttributeValue {
	return starlark.StringListValue{Values: labels}
}

// reparse writes a file and parses it again, returning its target.
func reparse(t *testing.T, written *starlark.File) *starlark.Target {
	t.Helper()
	f, err := starlark.Parse("rules.star", written.Write())
	if err != nil {
		t.Fatal(err)
	}
	return f.Targets[0]
}

// sameSet reports whether a and b hold the same labels, in any order.
func sameSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for _, s := range a {
		if !slices.Contains(b, s) {
			return false
		}
	}
	return true
}
