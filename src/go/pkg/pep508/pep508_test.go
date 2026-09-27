package pep508

import (
	_ "embed"
	"encoding/json"
	"reflect"
	"sort"
	"testing"
)

// The test cases pydeps-gen (src/cmd/pydeps-gen) runs too.
//
//go:embed testdata/pep508-vectors.json
var vectorsJSON []byte

type vectors struct {
	Environments map[string]Env `json:"environments"`
	Markers      []struct {
		Marker string   `json:"marker"`
		Holds  []string `json:"holds"`
	} `json:"markers"`
	Requirements []struct {
		Requirement string `json:"requirement"`
		Want        struct {
			Name   string   `json:"name"`
			Extras []string `json:"extras"`
			Marker string   `json:"marker"`
		} `json:"want"`
	} `json:"requirements"`
	Invalid []string `json:"invalid"`
}

func load(t *testing.T) vectors {
	t.Helper()
	var v vectors
	if err := json.Unmarshal(vectorsJSON, &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestMarkers(t *testing.T) {
	v := load(t)
	for _, c := range v.Markers {
		m, err := ParseMarker(c.Marker)
		if err != nil {
			t.Errorf("ParseMarker(%s): %v", c.Marker, err)
			continue
		}
		holds := []string{}
		for name, env := range v.Environments {
			if m.Evaluate(env) {
				holds = append(holds, name)
			}
		}
		sort.Strings(holds)
		want := append([]string{}, c.Holds...)
		sort.Strings(want)
		if !reflect.DeepEqual(holds, want) {
			t.Errorf("%s holds on %v, want %v", c.Marker, holds, want)
		}
	}
}

func TestRequirements(t *testing.T) {
	v := load(t)
	for _, c := range v.Requirements {
		req, err := ParseRequirement(c.Requirement)
		if err != nil {
			t.Errorf("ParseRequirement(%s): %v", c.Requirement, err)
			continue
		}
		marker := ""
		if req.Marker != nil {
			marker = req.Marker.Text
		}
		extras := req.Extras
		if extras == nil {
			extras = []string{}
		}
		if req.Name != c.Want.Name || !reflect.DeepEqual(extras, c.Want.Extras) || marker != c.Want.Marker {
			t.Errorf("%s = %q %v %q, want %q %v %q", c.Requirement, req.Name, extras, marker, c.Want.Name, c.Want.Extras, c.Want.Marker)
		}
	}
	for _, s := range v.Invalid {
		if _, err := ParseRequirement(s); err == nil {
			t.Errorf("ParseRequirement(%q) succeeded, want an error", s)
		}
	}
}

func TestMarkerVariables(t *testing.T) {
	m, err := ParseMarker(`sys_platform == "linux" and (python_version < "3.10" or extra == "x")`)
	if err != nil {
		t.Fatal(err)
	}
	if got, want := m.Variables(), []string{"extra", "python_version", "sys_platform"}; !reflect.DeepEqual(got, want) {
		t.Errorf("variables = %v, want %v", got, want)
	}
}

func TestPlatformEnv(t *testing.T) {
	env, ok := PlatformEnv("linux", "arm64", "3.13.12")
	if !ok || env["sys_platform"] != "linux" || env["platform_machine"] != "aarch64" || env["python_version"] != "3.13" {
		t.Errorf("linux-arm64 env = %v", env)
	}
	env, ok = PlatformEnv("macos", "arm64", "3.13.12")
	if !ok || env["sys_platform"] != "darwin" || env["platform_machine"] != "arm64" || env["platform_system"] != "Darwin" {
		t.Errorf("macos-arm64 env = %v", env)
	}
	if _, ok := PlatformEnv("windows", "x86_64", "3.13.12"); ok {
		t.Error("windows has an env, want none")
	}
}
