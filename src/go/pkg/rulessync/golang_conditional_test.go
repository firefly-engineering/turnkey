package rulessync

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// goConditionalFixture is a Go module whose packages depend on the
// platform and on build tags, with sync.toml allowing the integration tag.
func goConditionalFixture(t *testing.T) string {
	t.Helper()
	if _, err := exec.LookPath("go"); err != nil {
		t.Skip("go not in PATH: the Go extractor runs go list")
	}
	t.Setenv("GOWORK", "off")
	t.Setenv("GOFLAGS", "")

	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		".turnkey/sync.toml": `[[languages]]
name = "go"
cell = "godeps"
deps_file = "go-deps.toml"

[conditions]
settings = "toolchains//conditions"
go_tags = ["integration"]

[[conditions.platforms]]
os = "linux"
cpu = "x86_64"

[[conditions.platforms]]
os = "linux"
cpu = "arm64"

[[conditions.platforms]]
os = "macos"
cpu = "x86_64"

[[conditions.platforms]]
os = "macos"
cpu = "arm64"
`,
		"go.mod":                   "module example.com/project\n\ngo 1.22\n",
		"pkg/common/common.go":     "package common\n",
		"pkg/inotify/inotify.go":   "package inotify\n",
		"pkg/harness/harness.go":   "package harness\n",
		"pkg/watch/watch.go":       "package watch\n\nimport _ \"example.com/project/pkg/common\"\n",
		"pkg/watch/watch_linux.go": "package watch\n\nimport _ \"example.com/project/pkg/inotify\"\n",
		"pkg/watch/it.go":          "//go:build integration\n\npackage watch\n\nimport _ \"example.com/project/pkg/harness\"\n",
		"pkg/watch/rules.star": `go_library(
    name = "watch",
    deps = [],
)
`,
		"pkg/fixtures/fixtures.go": "package fixtures\n",
		"pkg/fixtures/it.go":       "//go:build integration\n\npackage fixtures\n\nimport _ \"example.com/project/pkg/harness\"\n",
		"pkg/fixtures/rules.star":  "go_library(\n    name = \"fixtures\",\n    deps = [],\n)\n",
		"cmd/it/main.go":           "//go:build integration\n\npackage main\n\nimport _ \"example.com/project/pkg/harness\"\n\nfunc main() {}\n",
		"cmd/it/rules.star": `go_binary(
    name = "it",
    build_tags = ["integration"],
    deps = [],
)
`,
	})
	return root
}

// syncGo syncs one rules.star of a fixture and returns what sync wrote.
func syncGo(t *testing.T, root, rel string) string {
	t.Helper()
	s, err := NewSyncer(Config{ProjectRoot: root, Force: true})
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, rel)
	result, err := s.SyncFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	out, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(out)
}

// A _linux.go file's import is a config//os:linux branch, whatever the
// host, and a //go:build integration file's import in a library is a
// branch on the tag's constraint: the OS and the tag combine on the
// toolchains cell's settings only when both matter.
func TestSyncFileGoPlatformAndTagDeps(t *testing.T) {
	root := goConditionalFixture(t)
	got := syncGo(t, root, "pkg/watch/rules.star")
	want := `go_library(
    name = "watch",
    deps = ["//pkg/common:common"] + select({
        "toolchains//conditions:linux-integration": [
            "//pkg/harness:harness",
            "//pkg/inotify:inotify",
        ],
        "toolchains//conditions:linux-no_integration": ["//pkg/inotify:inotify"],
        "toolchains//conditions:macos-integration": ["//pkg/harness:harness"],
        "toolchains//conditions:macos-no_integration": [],
    }),
)
`
	if got != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", got, want)
	}
}

// A library's tagged import alone is a branch on the tag's constraint.
func TestSyncFileGoTagDeps(t *testing.T) {
	root := goConditionalFixture(t)
	got := syncGo(t, root, "pkg/fixtures/rules.star")
	want := `go_library(
    name = "fixtures",
    deps = select({
        "prelude//go/tags/constraints:integration[set]": ["//pkg/harness:harness"],
        "prelude//go/tags/constraints:integration[unset]": [],
    }),
)
`
	if got != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", got, want)
	}
}

// A binary is built with its build_tags, literally: the tagged import is a
// plain dep.
func TestSyncFileGoBinaryBuildTags(t *testing.T) {
	root := goConditionalFixture(t)
	got := syncGo(t, root, "cmd/it/rules.star")
	want := `go_binary(
    name = "it",
    build_tags = ["integration"],
    deps = ["//pkg/harness:harness"],
)
`
	if got != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", got, want)
	}
}

// Go sync doesn't depend on the host: pretending to be each platform gives
// the same file.
func TestSyncFileGoIsHostIndependent(t *testing.T) {
	var outputs []string
	for _, host := range []struct{ goos, goarch string }{
		{"linux", "amd64"}, {"linux", "arm64"}, {"darwin", "amd64"}, {"darwin", "arm64"},
	} {
		root := goConditionalFixture(t)
		t.Setenv("GOOS", host.goos)
		t.Setenv("GOARCH", host.goarch)
		outputs = append(outputs, syncGo(t, root, "pkg/watch/rules.star"))
	}
	for i, out := range outputs[1:] {
		if out != outputs[0] {
			t.Errorf("host %d wrote:\n%s\nhost 0 wrote:\n%s", i+1, out, outputs[0])
		}
	}
}
