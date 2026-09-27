package rulessync

import (
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/mapper"
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

	changed := result.applyDeps(target, "deps", []string{"//pkg/y:y"}, []string{"example.com/unknown"}, nil)
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

	if result.applyDeps(target, "deps", nil, []string{"example.com/unknown"}, nil) {
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

	if !result.applyDeps(target, "deps", []string{"//pkg/y:y"}, nil, nil) {
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

// Deps given as an expression rather than a list of labels are not synced:
// rewriting them would replace the expression with a flat list.
func TestHasSyncableDeps(t *testing.T) {
	for _, tc := range []struct {
		deps string
		want bool
	}{
		{`["//a:a"]`, true},
		{"[\n        # turnkey:auto-start\n        \"//a:a\",\n        # turnkey:auto-end\n    ]", true},
		{`_DEPS`, false},
		{`_DEPS + ["//a:a"]`, false},
		{`["//a:a"] if X else []`, false},
	} {
		target := parseTarget(t, "rust_library(\n    name = \"lib\",\n    deps = "+tc.deps+",\n)\n")
		if got := hasSyncableDeps(target, "deps"); got != tc.want {
			t.Errorf("hasSyncableDeps(deps = %s) = %v, want %v", tc.deps, got, tc.want)
		}
	}
	if !hasSyncableDeps(parseTarget(t, "rust_library(name = \"lib\")\n"), "deps") {
		t.Error("target without deps is not syncable, want syncable")
	}
}

// A target already holding exactly the mapped deps is left as written,
// whatever their order.
func TestApplyDepsIgnoresOrder(t *testing.T) {
	target := parseTarget(t, `rust_library(
    name = "lib",
    deps = [
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
    ],
)
`)
	var result SyncResult
	mapped := []string{
		"rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
		"rustdeps//vendor/tree-sitter:tree-sitter",
	}
	if result.applyDeps(target, "deps", mapped, nil, nil) || len(result.Changes) != 0 {
		t.Errorf("reordering alone changed the target: %+v", result.Changes)
	}
}

// An existing label pinning a version of a mapped target satisfies it.
func TestApplyDepsKeepsVersionedLabel(t *testing.T) {
	target := parseTarget(t, `rust_library(
    name = "lib",
    deps = ["rustdeps//vendor/tokio@1.50.0:tokio"],
)
`)
	var result SyncResult
	if result.applyDeps(target, "deps", []string{"rustdeps//vendor/tokio:tokio"}, nil, nil) {
		t.Errorf("versioned label replaced: %+v", result.Changes)
	}
}

// An existing dep in the package of an unsynced dep is neither removed
// nor reported as kept, and an unsynced dep is never added.
func TestApplyDepsLeavesUnsyncedDeps(t *testing.T) {
	target := parseTarget(t, `rust_binary(
    name = "bin",
    deps = [
        "//src/rust/composition:composition-full",
        "rustdeps//vendor/log:log",
    ],
)
`)
	var result SyncResult
	if result.applyDeps(target, "deps", []string{"rustdeps//vendor/log:log"}, nil, []string{"//src/rust/composition:composition"}) {
		t.Errorf("target changed: %+v", result.Changes)
	}
	if len(result.Changes) != 0 {
		t.Errorf("changes = %+v, want none", result.Changes)
	}

	target = parseTarget(t, "rust_binary(\n    name = \"bin\",\n    deps = [],\n)\n")
	result = SyncResult{}
	result.applyDeps(target, "deps", nil, nil, []string{"//src/rust/composition:composition"})
	if got := target.GetDeps(); len(got) != 0 {
		t.Errorf("deps = %v, want unsynced dep not added", got)
	}
}

// A member's imports of its own modules map to its own target, which is
// not a dep of it.
func TestFilterSelfReference(t *testing.T) {
	self := computeSelfTarget("/repo/src/python/buck", "/repo")
	deps := []mapper.MappedDep{
		{Target: "//src/python/buck:buck", ImportPath: "turnkey.buck.generator"},
		{Target: "//src/python/cfg:cfg", ImportPath: "turnkey.cfg"},
	}
	got := mapper.DepsToTargets(filterSelfReference(deps, self))
	if want := []string{"//src/python/cfg:cfg"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
}

// A dep in a turnkey:preserve section is never removed, reported or copied
// into the auto-managed section.
func TestApplyDepsHonoursPreserveSection(t *testing.T) {
	src := `rust_binary(
    name = "bin",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//native:lib",
        # turnkey:preserve-end
    ],
)
`
	target := parseTarget(t, src)
	var result SyncResult
	if !result.applyDeps(target, "deps", []string{"//b:b"}, nil, nil) {
		t.Fatal("target not changed")
	}
	if got, want := target.GetAutoDeps(), []string{"//b:b"}; !reflect.DeepEqual(got, want) {
		t.Errorf("auto deps = %v, want %v", got, want)
	}
	if got, want := target.GetPreservedDeps(), []string{"//native:lib"}; !reflect.DeepEqual(got, want) {
		t.Errorf("preserved deps = %v, want %v", got, want)
	}
	want := []TargetChange{{Target: "bin", Added: []string{"//b:b"}, Removed: []string{"//a:a"}}}
	if !reflect.DeepEqual(result.Changes, want) {
		t.Errorf("changes = %+v, want %+v", result.Changes, want)
	}

	// Mapped deps that match: nothing to do.
	target = parseTarget(t, src)
	result = SyncResult{}
	if result.applyDeps(target, "deps", []string{"//a:a"}, nil, nil) || len(result.Changes) != 0 {
		t.Errorf("unchanged target reported changes: %+v", result.Changes)
	}
}

// A language whose deps go in another attribute (TypeScript's npm_deps)
// syncs that attribute and leaves deps alone.
func TestApplyDepsToOtherAttribute(t *testing.T) {
	target := parseTarget(t, `typescript_binary(
    name = "app",
    deps = ["//lib:lib"],
    npm_deps = ["jsdeps//:left-pad"],
)
`)
	var result SyncResult
	if !result.applyDeps(target, "npm_deps", []string{"jsdeps//:lodash"}, nil, nil) {
		t.Fatal("target not changed")
	}
	if got, want := target.GetLabels("npm_deps"), []string{"jsdeps//:lodash"}; !reflect.DeepEqual(got, want) {
		t.Errorf("npm_deps = %v, want %v", got, want)
	}
	if got, want := target.GetDeps(), []string{"//lib:lib"}; !reflect.DeepEqual(got, want) {
		t.Errorf("deps = %v, want %v", got, want)
	}
}

// In one file, an ordinary target is synced, a target opted out with
// "# turnkey:no-sync" is left alone and reported as opted out, and one
// whose deps sync can't read is reported as unreadable.
func TestSyncFileOptOutAndUnreadable(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"Cargo.toml": "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nanyhow = \"1\"\n",
		"rust-deps.toml": `[deps."anyhow@1.0.0"]
name = "anyhow"
`,
		"crates/lib/Cargo.toml": "[package]\nname = \"lib\"\n\n[dependencies]\nanyhow.workspace = true\n",
		"crates/lib/rules.star": `load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "lib",
    deps = [],
)

# turnkey:no-sync
rust_library(
    name = "lib-custom",
    deps = ["//somewhere:else"],
)

_DEPS = ["rustdeps//vendor/anyhow:anyhow"]

rust_test(
    name = "lib-test",
    deps = _DEPS,
)
`,
	})

	s, err := NewSyncer(Config{ProjectRoot: root, Force: true, DryRun: true})
	if err != nil {
		t.Fatal(err)
	}
	result, err := s.SyncFile(filepath.Join(root, "crates/lib/rules.star"))
	if err != nil {
		t.Fatal(err)
	}

	wantChanges := []TargetChange{{Target: "lib", Added: []string{"rustdeps//vendor/anyhow:anyhow"}}}
	if !reflect.DeepEqual(result.Changes, wantChanges) {
		t.Errorf("changes = %+v, want %+v", result.Changes, wantChanges)
	}
	if want := []string{"lib-custom"}; !reflect.DeepEqual(result.OptedOut, want) {
		t.Errorf("opted out = %v, want %v", result.OptedOut, want)
	}
	wantUnreadable := []UnreadableTarget{{Target: "lib-test", Attribute: "deps"}}
	if !reflect.DeepEqual(result.Unreadable, wantUnreadable) {
		t.Errorf("unreadable = %+v, want %+v", result.Unreadable, wantUnreadable)
	}
}
