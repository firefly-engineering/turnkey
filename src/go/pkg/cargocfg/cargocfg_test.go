package cargocfg

import (
	_ "embed"
	"encoding/json"
	"reflect"
	"slices"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// The test cases turnkey.cfg (src/python/cfg) runs too.
//
//go:embed testdata/cfg-vectors.json
var vectorsJSON []byte

type vectors struct {
	Platforms []conditions.Platform `json:"platforms"`
	Cfg       []vector              `json:"cfg"`
	Triples   []vector              `json:"triples"`
}

type vector struct {
	Spec    string   `json:"spec"`
	Matches []string `json:"matches"`
}

func TestSharedVectors(t *testing.T) {
	var v vectors
	if err := json.Unmarshal(vectorsJSON, &v); err != nil {
		t.Fatal(err)
	}
	for _, c := range append(v.Cfg, v.Triples...) {
		spec, err := Parse(c.Spec)
		if err != nil {
			t.Errorf("Parse(%s): %v", c.Spec, err)
			continue
		}
		matches := []string{}
		for _, p := range v.Platforms {
			target, ok := ForPlatform(p)
			if !ok {
				t.Fatalf("no target for %s", p)
			}
			if spec.Matches(target) {
				matches = append(matches, p.String())
			}
		}
		want := append([]string{}, c.Matches...)
		slices.Sort(want)
		slices.Sort(matches)
		if got := matches; !reflect.DeepEqual(got, want) {
			t.Errorf("%s matches %v, want %v", c.Spec, got, want)
		}
	}
}

func TestParseErrors(t *testing.T) {
	for _, spec := range []string{
		`cfg(`,
		`cfg(target_os = )`,
		`cfg(target_os = "linux"`,
		`cfg(not(unix, windows))`,
		`cfg(maybe(unix))`,
		`cfg(unix) extra`,
		`cfg(target_os = "linux)`,
		`not a triple`,
		``,
	} {
		if _, err := Parse(spec); err == nil {
			t.Errorf("Parse(%q) succeeded, want an error", spec)
		}
	}
}

func TestForPlatformUnknown(t *testing.T) {
	if _, ok := ForPlatform(conditions.Platform{OS: "windows", CPU: "x86_64"}); ok {
		t.Error("windows has a target, want none")
	}
}
