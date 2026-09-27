package pep508

import (
	_ "embed"
	"encoding/json"
	"reflect"
	"sort"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
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

// A configuration's platform, in Buck2's names, sets the platform
// variables, and the Python version sets the version ones.
func TestEnvFor(t *testing.T) {
	cases := []struct {
		os, cpu                      string
		sysPlatform, system, machine string
	}{
		{"linux", "x86_64", "linux", "Linux", "x86_64"},
		{"linux", "arm64", "linux", "Linux", "aarch64"},
		{"macos", "x86_64", "darwin", "Darwin", "x86_64"},
		{"macos", "arm64", "darwin", "Darwin", "arm64"},
	}
	for _, c := range cases {
		config := conditions.Configuration{conditions.OS: c.os, conditions.CPU: c.cpu}
		env, ok := EnvFor(config, "3.13.12", "Test_Extra")
		want := Env{
			"os_name":                        "posix",
			"sys_platform":                   c.sysPlatform,
			"platform_system":                c.system,
			"platform_machine":               c.machine,
			"implementation_name":            "cpython",
			"platform_python_implementation": "CPython",
			"python_full_version":            "3.13.12",
			"implementation_version":         "3.13.12",
			"python_version":                 "3.13",
			"extra":                          "Test_Extra",
		}
		if !ok || !reflect.DeepEqual(env, want) {
			t.Errorf("EnvFor(%s) = %v, %v; want %v, true", config, env, ok, want)
		}
	}
}

// Without a platform or a Python version, the environment holds what it
// still knows, and says which markers it can't decide.
func TestEnvForFallbacks(t *testing.T) {
	env, ok := EnvFor(conditions.Configuration{}, "3.12.1", "")
	want := Env{
		"implementation_name":            "cpython",
		"platform_python_implementation": "CPython",
		"python_full_version":            "3.12.1",
		"implementation_version":         "3.12.1",
		"python_version":                 "3.12",
		"extra":                          "",
	}
	if !ok || !reflect.DeepEqual(env, want) {
		t.Errorf("EnvFor without a platform = %v, %v; want %v, true", env, ok, want)
	}

	env, ok = EnvFor(conditions.Configuration{conditions.OS: "linux", conditions.CPU: "x86_64"}, "", "x")
	if !ok || env["sys_platform"] != "linux" || env["extra"] != "x" {
		t.Errorf("EnvFor without a Python version = %v, %v", env, ok)
	}
	for _, v := range []string{"python_version", "python_full_version", "implementation_version"} {
		if _, has := env[v]; has {
			t.Errorf("EnvFor without a Python version holds %s", v)
		}
	}

	if _, ok := EnvFor(conditions.Configuration{conditions.OS: "windows", conditions.CPU: "x86_64"}, "3.13.12", ""); ok {
		t.Error("EnvFor(windows) reports a known platform")
	}
}

// An environment decides a marker if it holds every variable the marker
// reads; the extra is always held, platform_release and platform_version
// never.
func TestEnvDecides(t *testing.T) {
	noPlatform, _ := EnvFor(conditions.Configuration{}, "3.13.12", "")
	noVersion, _ := EnvFor(conditions.Configuration{conditions.OS: "macos", conditions.CPU: "arm64"}, "", "")
	full, _ := EnvFor(conditions.Configuration{conditions.OS: "macos", conditions.CPU: "arm64"}, "3.13.12", "")
	cases := []struct {
		marker                      string
		noPlatform, noVersion, full bool
	}{
		{`extra == "x"`, true, true, true},
		{`implementation_name == "cpython"`, true, true, true},
		{`python_version < "3.11"`, true, false, true},
		{`sys_platform == "linux"`, false, true, true},
		{`sys_platform == "linux" and python_version < "3.11"`, false, false, true},
		{`platform_release >= "5"`, false, false, false},
		{`platform_version == "x"`, false, false, false},
	}
	for _, c := range cases {
		m, err := ParseMarker(c.marker)
		if err != nil {
			t.Fatal(err)
		}
		for _, e := range []struct {
			name string
			env  Env
			want bool
		}{{"no platform", noPlatform, c.noPlatform}, {"no version", noVersion, c.noVersion}, {"full", full, c.full}} {
			if got := e.env.Decides(m); got != e.want {
				t.Errorf("%s env decides %q = %v, want %v", e.name, c.marker, got, e.want)
			}
		}
	}
	if !full.Decides(nil) {
		t.Error("an env doesn't decide the nil marker")
	}
}

// The extra is compared normalized, as the extra EnvFor is given.
func TestEnvForExtra(t *testing.T) {
	m, err := ParseMarker(`extra == "test-extra"`)
	if err != nil {
		t.Fatal(err)
	}
	with, _ := EnvFor(conditions.Configuration{}, "3.13.12", "Test_Extra")
	without, _ := EnvFor(conditions.Configuration{}, "3.13.12", "")
	if !m.Evaluate(with) || m.Evaluate(without) {
		t.Errorf("extra marker: with = %v, without = %v; want true, false", m.Evaluate(with), m.Evaluate(without))
	}
}
