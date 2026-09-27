package pydepscell

import (
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/pelletier/go-toml/v2"
)

var cfg = Config{
	Settings: "toolchains//conditions",
	Platforms: []conditions.Platform{
		{OS: "linux", CPU: "x86_64"},
		{OS: "linux", CPU: "arm64"},
		{OS: "macos", CPU: "x86_64"},
		{OS: "macos", CPU: "arm64"},
	},
	PythonVersion: "3.13.12",
}

const depsTOML = `
[deps.pytest]
dependencies = [
  { name = "iniconfig" },
  { name = "colorama", marker = "sys_platform == 'win32'" },
  { name = "exceptiongroup", marker = "python_version < '3.11'" },
  { name = "uvloop", marker = "sys_platform == 'linux'" },
  { name = "notvendored" },
]
requested_extras = ["testing"]

[deps.pytest.extras]
testing = [{ name = "hypothesis", marker = "extra == 'testing'" }]
other = [{ name = "xmlschema" }]

[deps.iniconfig]
[deps.colorama]
[deps.exceptiongroup]
[deps.uvloop]
[deps.hypothesis]
[deps.xmlschema]
`

func render(t *testing.T, name string) string {
	t.Helper()
	var deps DepsFile
	if err := toml.Unmarshal([]byte(depsTOML), &deps); err != nil {
		t.Fatal(err)
	}
	var b strings.Builder
	if err := Render(&b, name, &deps, cfg); err != nil {
		t.Fatal(err)
	}
	return b.String()
}

// Markers are evaluated per platform and for the toolchain's Python: a
// Linux-only dependency is a config//os:linux branch, Windows-only and
// old-Python ones are dropped, and a requested extra's deps are in.
func TestRenderEvaluatesMarkers(t *testing.T) {
	got := render(t, "pytest")
	want := `    deps = [
        "//vendor/hypothesis:hypothesis",
        "//vendor/iniconfig:iniconfig",
    ] + select({
        "config//os:linux": ["//vendor/uvloop:uvloop"],
        "config//os:macos": [],
    }),
`
	if !strings.Contains(got, want) {
		t.Errorf("rules.star:\n%s\nwant deps:\n%s", got, want)
	}
	for _, absent := range []string{"colorama", "exceptiongroup", "notvendored", "xmlschema", "DEFAULT"} {
		if strings.Contains(got, absent) {
			t.Errorf("rules.star has %s:\n%s", absent, got)
		}
	}
}

func TestRenderWithoutDeps(t *testing.T) {
	got := render(t, "iniconfig")
	if strings.Contains(got, "deps =") || !strings.Contains(got, `name = "iniconfig"`) {
		t.Errorf("rules.star:\n%s", got)
	}
}
