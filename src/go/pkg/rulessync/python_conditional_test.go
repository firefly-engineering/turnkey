package rulessync

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// A uv workspace member's platform marker is a select() on the OS, a marker
// the Python toolchain doesn't satisfy drops the dep, and a target built
// with an extra gets the extra's deps.
func TestSyncFilePythonMarkersAndExtras(t *testing.T) {
	for _, tool := range []string{"deps-extract", "python3"} {
		if _, err := exec.LookPath(tool); err != nil {
			t.Skipf("%s not in PATH", tool)
		}
	}
	out, err := exec.Command("python3", "-c", "import sys; print(sys.version_info >= (3, 10))").Output()
	if err != nil || string(out) != "True\n" {
		t.Skip("needs a Python >= 3.10 toolchain")
	}

	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"pyproject.toml": `[project]
name = "root"

[tool.uv.workspace]
members = ["src/app"]
`,
		"python-deps.toml": `schema_version = 2

[deps.requests]
[deps.oldlib]
[deps.extradep]
`,
		"src/app/pyproject.toml": `[project]
name = "app"
dependencies = [
    "requests ; sys_platform == \"linux\"",
    "oldlib ; python_version < \"3.10\"",
]

[project.optional-dependencies]
x = ["extradep"]
`,
		"src/app/app/__init__.py": "import requests\nimport oldlib\n",
		"src/app/rules.star": `python_library(
    name = "app",
    deps = [],
)

python_library(
    name = "app-x",
    extras = ["x"],
    deps = [],
)
`,
	})
	s, err := NewSyncer(Config{
		ProjectRoot: root,
		Force:       true,
		Sync:        testSync(defaultPlatforms),
	})
	if err != nil {
		t.Fatal(err)
	}
	rulesPath := filepath.Join(root, "src/app/rules.star")
	result, err := s.SyncFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	got, err := os.ReadFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	want := `python_library(
    name = "app",
    deps = select({
        "config//os:linux": ["pydeps//vendor/requests:requests"],
        "config//os:macos": [],
    }),
)

python_library(
    name = "app-x",
    extras = ["x"],
    deps = ["pydeps//vendor/extradep:extradep"] + select({
        "config//os:linux": ["pydeps//vendor/requests:requests"],
        "config//os:macos": [],
    }),
)
`
	if string(got) != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", got, want)
	}
}
