package conditions

import (
	_ "embed"
	"encoding/json"
	"reflect"
	"testing"
)

// The test cases turnkey.cfg (src/python/cfg) and nix/buck2/platforms.nix
// (the split-vectors flake check) run too. The test only uses the
// package's exported interface, as a mirror sees it.
//
//go:embed testdata/split-vectors.json
var splitVectorsJSON []byte

type splitVector struct {
	Name       string     `json:"name"`
	Platforms  []Platform `json:"platforms"`
	Settings   string     `json:"settings"`
	Dimensions []string   `json:"dimensions"`
	Labels     []struct {
		When   Configuration `json:"when"`
		Labels []string      `json:"labels"`
	} `json:"labels"`
	Common   []string `json:"common"`
	Branches []struct {
		Key    string   `json:"key"`
		Labels []string `json:"labels"`
	} `json:"branches"`
}

func TestSplitVectors(t *testing.T) {
	var doc struct {
		Cases []splitVector `json:"cases"`
	}
	if err := json.Unmarshal(splitVectorsJSON, &doc); err != nil {
		t.Fatal(err)
	}
	for _, c := range doc.Cases {
		t.Run(c.Name, func(t *testing.T) {
			space := NewSpace(c.Platforms, c.Settings).WithDimensions(c.Dimensions)
			got := space.Split(func(config Configuration) []string {
				var labels []string
				for _, rule := range c.Labels {
					if config.Includes(rule.When) {
						labels = append(labels, rule.Labels...)
					}
				}
				return labels
			})
			if !sameList(got.Common, c.Common) {
				t.Errorf("common = %v, want %v", got.Common, c.Common)
			}
			if len(got.Branches) != len(c.Branches) {
				t.Fatalf("branches = %+v, want %+v", got.Branches, c.Branches)
			}
			for i, b := range got.Branches {
				if b.Key != c.Branches[i].Key || !sameList(b.Labels, c.Branches[i].Labels) {
					t.Errorf("branch %d = %+v, want %+v", i, b, c.Branches[i])
				}
			}
		})
	}
}

// sameList reports whether a and b hold the same labels in the same order,
// nil and empty alike.
func sameList(a, b []string) bool {
	return len(a) == len(b) && (len(a) == 0 || reflect.DeepEqual(a, b))
}
