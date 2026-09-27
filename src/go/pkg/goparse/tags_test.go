package goparse

import (
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// A configuration sets the tags whose dimension it sets on, and no other.
func TestConfigTags(t *testing.T) {
	config := conditions.Configuration{
		conditions.OS:            "linux",
		TagDimension("b").Name:   conditions.Set,
		TagDimension("a").Name:   conditions.Set,
		TagDimension("off").Name: conditions.Unset,
	}
	if got, want := ConfigTags(config), []string{"a", "b"}; !reflect.DeepEqual(got, want) {
		t.Errorf("ConfigTags = %v, want %v", got, want)
	}
}

// A tag's dimension is keyed on the prelude's constraint for it.
func TestTagDimensionKeys(t *testing.T) {
	space := conditions.NewSpace(nil, "").WithDimensions([]conditions.OnOff{TagDimension("integration")})
	split := space.Split(func(c conditions.Configuration) []string {
		return ConfigTags(c)
	})
	want := []conditions.Branch{
		{Key: "prelude//go/tags/constraints:integration[set]", Labels: []string{"integration"}},
		{Key: "prelude//go/tags/constraints:integration[unset]"},
	}
	if !reflect.DeepEqual(split.Branches, want) {
		t.Errorf("branches = %+v, want %+v", split.Branches, want)
	}
}
