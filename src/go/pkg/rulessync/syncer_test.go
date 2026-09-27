package rulessync

import (
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// writeFiles writes files (relative path -> content) under root.
func writeFiles(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for rel, content := range files {
		path := filepath.Join(root, rel)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}
}

// A go_binary's deps follow its imports, as a go_library's do: the internal
// package it imports is added and one it no longer imports is removed.
func TestSyncFileGoBinary(t *testing.T) {
	if _, err := exec.LookPath("go"); err != nil {
		t.Skip("go not in PATH: the Go extractor runs go list")
	}
	t.Setenv("GOWORK", "off")
	t.Setenv("GOFLAGS", "")

	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"go.mod":             "module example.com/project\n\ngo 1.22\n",
		"pkg/greet/greet.go": "package greet\n\nfunc Hello() string { return \"hello\" }\n",
		"pkg/old/old.go":     "package old\n",
		"cmd/hello/main.go": `package main

import (
	"fmt"

	"example.com/project/pkg/greet"
)

func main() { fmt.Println(greet.Hello()) }
`,
		"cmd/hello/rules.star": `load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "hello",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "//pkg/old:old",
        # turnkey:auto-end
    ],
)
`,
	})

	s, err := NewSyncer(Config{ProjectRoot: root, Force: true})
	if err != nil {
		t.Fatal(err)
	}
	rulesPath := filepath.Join(root, "cmd/hello/rules.star")
	result, err := s.SyncFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	if !result.Updated {
		t.Error("rules.star not updated")
	}

	f, err := starlark.ParseFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"//pkg/greet:greet"}
	if got := f.Targets[0].GetDeps(); !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
}

// Each target whose deps change gets its own entry in the result, test
// targets included, instead of the last one overwriting the others.
func TestSyncFileReportsEachTarget(t *testing.T) {
	if _, err := exec.LookPath("go"); err != nil {
		t.Skip("go not in PATH: the Go extractor runs go list")
	}
	t.Setenv("GOWORK", "off")
	t.Setenv("GOFLAGS", "")

	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"go.mod":             "module example.com/project\n\ngo 1.22\n",
		"pkg/greet/greet.go": "package greet\n\nfunc Hello() string { return \"hello\" }\n",
		"pkg/check/check.go": "package check\n\nfunc OK() bool { return true }\n",
		"pkg/old/old.go":     "package old\n",
		"pkg/lib/lib.go": `package lib

import "example.com/project/pkg/greet"

func Hi() string { return greet.Hello() }
`,
		"pkg/lib/lib_test.go": `package lib

import (
	"testing"

	"example.com/project/pkg/check"
)

func TestHi(t *testing.T) { _ = check.OK() }
`,
		"pkg/lib/rules.star": `load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "lib",
    srcs = ["lib.go"],
    deps = [
        # turnkey:auto-start
        "//pkg/old:old",
        # turnkey:auto-end
    ],
)

go_test(
    name = "lib_test",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        # turnkey:auto-end
    ],
)
`,
	})

	s, err := NewSyncer(Config{ProjectRoot: root, Force: true, DryRun: true})
	if err != nil {
		t.Fatal(err)
	}
	result, err := s.SyncFile(filepath.Join(root, "pkg/lib/rules.star"))
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}

	want := []TargetChange{
		{Target: "lib", Added: []string{"//pkg/greet:greet"}, Removed: []string{"//pkg/old:old"}},
		{Target: "lib_test", Added: []string{"//pkg/greet:greet", "//pkg/check:check"}},
	}
	if !reflect.DeepEqual(result.Changes, want) {
		t.Errorf("changes = %+v, want %+v", result.Changes, want)
	}
}

// parseTarget parses a rules.star source and returns its only target.
func parseTarget(t *testing.T, src string) *starlark.Target {
	t.Helper()
	f, err := starlark.Parse("rules.star", []byte(src))
	if err != nil {
		t.Fatal(err)
	}
	if len(f.Targets) != 1 {
		t.Fatalf("got %d targets, want 1", len(f.Targets))
	}
	return f.Targets[0]
}

const libWithX = `go_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//pkg/x:x",
        # turnkey:auto-end
    ],
)
`

// With unmapped imports the mapped deps are incomplete: an existing dep the
// mapper didn't return is kept, and a newly mapped dep is still added.
func TestApplyDepsKeepsDepsWhenUnmapped(t *testing.T) {
	target := parseTarget(t, libWithX)
	var result SyncResult

	changed := result.applyDeps(target, []string{"//pkg/y:y"}, []string{"example.com/unknown"})
	if !changed {
		t.Error("target not changed, want //pkg/y:y added")
	}
	if got, want := target.GetDeps(), []string{"//pkg/x:x", "//pkg/y:y"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
	want := []TargetChange{{
		Target:   "lib",
		Added:    []string{"//pkg/y:y"},
		Kept:     []string{"//pkg/x:x"},
		Unmapped: []string{"example.com/unknown"},
	}}
	if !reflect.DeepEqual(result.Changes, want) {
		t.Errorf("changes = %+v, want %+v", result.Changes, want)
	}
}

// A kept dep is reported even when nothing else about the target changes.
func TestApplyDepsReportsKeptWithoutChange(t *testing.T) {
	target := parseTarget(t, libWithX)
	var result SyncResult

	if result.applyDeps(target, nil, []string{"example.com/unknown"}) {
		t.Error("target changed, want deps untouched")
	}
	if got, want := target.GetDeps(), []string{"//pkg/x:x"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
	if len(result.Changes) != 1 || !reflect.DeepEqual(result.Changes[0].Kept, []string{"//pkg/x:x"}) {
		t.Errorf("changes = %+v, want //pkg/x:x kept", result.Changes)
	}
}

// Without unmapped imports the mapped deps are complete, so a dep the
// mapper didn't return is removed.
func TestApplyDepsRemovesWhenAllMapped(t *testing.T) {
	target := parseTarget(t, libWithX)
	var result SyncResult

	if !result.applyDeps(target, []string{"//pkg/y:y"}, nil) {
		t.Error("target not changed")
	}
	if got, want := target.GetDeps(), []string{"//pkg/y:y"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
	want := []TargetChange{{Target: "lib", Added: []string{"//pkg/y:y"}, Removed: []string{"//pkg/x:x"}}}
	if !reflect.DeepEqual(result.Changes, want) {
		t.Errorf("changes = %+v, want %+v", result.Changes, want)
	}
}
