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
