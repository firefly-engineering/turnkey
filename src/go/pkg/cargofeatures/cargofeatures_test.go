package cargofeatures

import (
	_ "embed"
	"encoding/json"
	"reflect"
	"testing"
)

// The test cases turnkey.cargo.features (src/python/cargo) runs too.
//
//go:embed testdata/activation-vectors.json
var vectorsJSON []byte

type vector struct {
	Name      string              `json:"name"`
	Features  map[string][]string `json:"features"`
	Optional  []string            `json:"optional"`
	Required  []string            `json:"required"`
	Requested []string            `json:"requested"`
	Remove    []string            `json:"remove"`
	Want      struct {
		Features     []string            `json:"features"`
		OptionalDeps []string            `json:"optional_deps"`
		DepFeatures  map[string][]string `json:"dep_features"`
	} `json:"want"`
}

func set(keys []string) map[string]bool {
	s := make(map[string]bool, len(keys))
	for _, k := range keys {
		s[k] = true
	}
	return s
}

func TestSharedVectors(t *testing.T) {
	var doc struct {
		Cases []vector `json:"cases"`
	}
	if err := json.Unmarshal(vectorsJSON, &doc); err != nil {
		t.Fatal(err)
	}
	for _, c := range doc.Cases {
		got := Activate(Crate{Features: c.Features, Optional: set(c.Optional), Required: set(c.Required)}, c.Requested, c.Remove)
		if !reflect.DeepEqual(nonNil(got.Features), nonNil(c.Want.Features)) {
			t.Errorf("%s: features = %v, want %v", c.Name, got.Features, c.Want.Features)
		}
		if !reflect.DeepEqual(nonNil(got.OptionalDeps), nonNil(c.Want.OptionalDeps)) {
			t.Errorf("%s: optional deps = %v, want %v", c.Name, got.OptionalDeps, c.Want.OptionalDeps)
		}
		if !reflect.DeepEqual(got.DepFeatures, c.Want.DepFeatures) {
			t.Errorf("%s: dep features = %v, want %v", c.Name, got.DepFeatures, c.Want.DepFeatures)
		}
	}
}

func nonNil(s []string) []string {
	if s == nil {
		return []string{}
	}
	return s
}
