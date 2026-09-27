package mapper

import (
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// A variant attribute is read as a label list is: in a configuration no
// branch applies to, a list followed by a select() is the list alone, and
// a concatenation holds each label once.
func TestReadVariantPlainPartWhenNoBranchApplies(t *testing.T) {
	f, err := starlark.Parse("rules.star", []byte(`rust_library(
    name = "lib",
    cargo_features = ["std", "alloc"] + select({"config//os:linux": ["alloc", "linux"]}),
    default_features = select({"config//os:linux": False}),
)
`))
	if err != nil {
		t.Fatal(err)
	}
	space := conditions.NewSpace([]conditions.Platform{{OS: "linux", CPU: "x86_64"}, {OS: "macos", CPU: "arm64"}}, "")
	variant, bad, ok := ReadVariant(f.Targets[0], cargoVariantAttributes, space)
	if !ok {
		t.Fatalf("%s is unreadable", bad)
	}
	for _, tc := range []struct {
		config conditions.Configuration
		want   map[string]starlark.AttributeValue
	}{
		{space.Configurations[0], map[string]starlark.AttributeValue{
			"cargo_features":   starlark.StringListValue{Values: []string{"std", "alloc", "linux"}},
			"default_features": starlark.BoolValue{Value: false},
		}},
		{space.Configurations[1], map[string]starlark.AttributeValue{
			"cargo_features": starlark.StringListValue{Values: []string{"std", "alloc"}},
		}},
	} {
		if got := variant(tc.config); !reflect.DeepEqual(got, tc.want) {
			t.Errorf("%s: variant = %#v, want %#v", tc.config, got, tc.want)
		}
	}
}
