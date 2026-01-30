package godeps

import (
	"testing"
)

func TestParseGoMod_SingleRequire(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/foo/bar v1.0.0
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 1 {
		t.Fatalf("expected 1 dep, got %d", len(deps))
	}
	if deps[0].ImportPath != "github.com/foo/bar" {
		t.Errorf("expected import path github.com/foo/bar, got %s", deps[0].ImportPath)
	}
	if deps[0].Version != "v1.0.0" {
		t.Errorf("expected version v1.0.0, got %s", deps[0].Version)
	}
	if deps[0].Indirect {
		t.Error("expected direct dependency, got indirect")
	}
}

func TestParseGoMod_RequireBlock(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require (
	github.com/foo/bar v1.0.0
	github.com/baz/qux v1.2.0
)
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 2 {
		t.Fatalf("expected 2 deps, got %d", len(deps))
	}
	// Should be sorted by import path
	if deps[0].ImportPath != "github.com/baz/qux" {
		t.Errorf("expected first dep github.com/baz/qux, got %s", deps[0].ImportPath)
	}
	if deps[1].ImportPath != "github.com/foo/bar" {
		t.Errorf("expected second dep github.com/foo/bar, got %s", deps[1].ImportPath)
	}
}

func TestParseGoMod_IndirectDependencies(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require (
	github.com/direct/dep v1.0.0
	github.com/indirect/dep v1.2.0 // indirect
)
`)
	t.Run("include indirect", func(t *testing.T) {
		opts := ParseOptions{IncludeIndirect: true}
		deps, err := ParseGoMod(input, opts)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(deps) != 2 {
			t.Fatalf("expected 2 deps, got %d", len(deps))
		}

		// Find the indirect dep
		var indirectDep *Dependency
		for i := range deps {
			if deps[i].ImportPath == "github.com/indirect/dep" {
				indirectDep = &deps[i]
				break
			}
		}
		if indirectDep == nil {
			t.Fatal("indirect dep not found")
		}
		if !indirectDep.Indirect {
			t.Error("expected Indirect=true for indirect dep")
		}
	})

	t.Run("exclude indirect", func(t *testing.T) {
		opts := ParseOptions{IncludeIndirect: false}
		deps, err := ParseGoMod(input, opts)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(deps) != 1 {
			t.Fatalf("expected 1 dep (indirect excluded), got %d", len(deps))
		}
		if deps[0].ImportPath != "github.com/direct/dep" {
			t.Errorf("expected github.com/direct/dep, got %s", deps[0].ImportPath)
		}
	})
}

func TestParseGoMod_EmptyModule(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 0 {
		t.Errorf("expected 0 deps, got %d", len(deps))
	}
}

func TestParseGoMod_MixedRequires(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/single/dep v1.0.0

require (
	github.com/block/dep1 v1.1.0
	github.com/block/dep2 v1.2.0
)

require github.com/another/single v1.3.0
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 4 {
		t.Fatalf("expected 4 deps, got %d", len(deps))
	}
}

func TestParseGoMod_Comments(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

// This is a comment before require
require (
	// Comment inside block
	github.com/foo/bar v1.0.0 // trailing comment
	github.com/baz/qux v1.1.0
)
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 2 {
		t.Fatalf("expected 2 deps, got %d", len(deps))
	}
}

func TestParseGoMod_PseudoVersion(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/foo/bar v0.0.0-20231215123456-abcdef123456
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 1 {
		t.Fatalf("expected 1 dep, got %d", len(deps))
	}
	if deps[0].Version != "v0.0.0-20231215123456-abcdef123456" {
		t.Errorf("expected pseudo-version, got %s", deps[0].Version)
	}
}

func TestParseGoMod_IncompatibleVersion(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/foo/bar v4.0.0+incompatible
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 1 {
		t.Fatalf("expected 1 dep, got %d", len(deps))
	}
	if deps[0].Version != "v4.0.0+incompatible" {
		t.Errorf("expected +incompatible version, got %s", deps[0].Version)
	}
}

func TestParseGoMod_LongModulePath(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/very/long/nested/module/path/here v1.0.0
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 1 {
		t.Fatalf("expected 1 dep, got %d", len(deps))
	}
	if deps[0].ImportPath != "github.com/very/long/nested/module/path/here" {
		t.Errorf("unexpected import path: %s", deps[0].ImportPath)
	}
}

func TestParseGoMod_InvalidSyntax(t *testing.T) {
	input := []byte(`this is not valid go.mod syntax at all`)
	_, err := ParseGoMod(input, DefaultParseOptions())
	if err == nil {
		t.Error("expected error for invalid syntax")
	}
}

func TestParseGoMod_Sorted(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require (
	github.com/zzz/last v1.0.0
	github.com/aaa/first v1.0.0
	github.com/mmm/middle v1.0.0
)
`)
	deps, err := ParseGoMod(input, DefaultParseOptions())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(deps) != 3 {
		t.Fatalf("expected 3 deps, got %d", len(deps))
	}
	// Verify sorted order
	expected := []string{"github.com/aaa/first", "github.com/mmm/middle", "github.com/zzz/last"}
	for i, exp := range expected {
		if deps[i].ImportPath != exp {
			t.Errorf("position %d: expected %s, got %s", i, exp, deps[i].ImportPath)
		}
	}
}

// go.sum parsing tests

func TestParseGoSum_SingleEntry(t *testing.T) {
	input := []byte(`github.com/foo/bar v1.0.0 h1:abcdef123456=
`)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(hashes) != 1 {
		t.Fatalf("expected 1 hash, got %d", len(hashes))
	}
	key := "github.com/foo/bar v1.0.0"
	if hashes[key] != "h1:abcdef123456=" {
		t.Errorf("expected h1:abcdef123456=, got %s", hashes[key])
	}
}

func TestParseGoSum_MultipleEntries(t *testing.T) {
	input := []byte(`github.com/foo/bar v1.0.0 h1:hash1=
github.com/foo/bar v1.0.0/go.mod h1:modhash=
github.com/baz/qux v2.0.0 h1:hash2=
github.com/baz/qux v2.0.0/go.mod h1:modhash2=
`)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should only have source hashes, not /go.mod hashes
	if len(hashes) != 2 {
		t.Fatalf("expected 2 hashes (excluding /go.mod), got %d", len(hashes))
	}
	if hashes["github.com/foo/bar v1.0.0"] != "h1:hash1=" {
		t.Error("missing or wrong hash for foo/bar")
	}
	if hashes["github.com/baz/qux v2.0.0"] != "h1:hash2=" {
		t.Error("missing or wrong hash for baz/qux")
	}
}

func TestParseGoSum_EmptyFile(t *testing.T) {
	input := []byte(``)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(hashes) != 0 {
		t.Errorf("expected 0 hashes, got %d", len(hashes))
	}
}

func TestParseGoSum_ExtraWhitespace(t *testing.T) {
	input := []byte(`
github.com/foo/bar v1.0.0 h1:hash=

github.com/baz/qux v2.0.0 h1:hash2=

`)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(hashes) != 2 {
		t.Fatalf("expected 2 hashes, got %d", len(hashes))
	}
}

func TestParseGoSum_MultipleVersions(t *testing.T) {
	input := []byte(`github.com/foo/bar v1.0.0 h1:hash1=
github.com/foo/bar v1.1.0 h1:hash2=
github.com/foo/bar v2.0.0 h1:hash3=
`)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(hashes) != 3 {
		t.Fatalf("expected 3 hashes, got %d", len(hashes))
	}
	if hashes["github.com/foo/bar v1.0.0"] != "h1:hash1=" {
		t.Error("wrong hash for v1.0.0")
	}
	if hashes["github.com/foo/bar v1.1.0"] != "h1:hash2=" {
		t.Error("wrong hash for v1.1.0")
	}
	if hashes["github.com/foo/bar v2.0.0"] != "h1:hash3=" {
		t.Error("wrong hash for v2.0.0")
	}
}

func TestParseGoSum_SkipsNonH1Hashes(t *testing.T) {
	input := []byte(`github.com/foo/bar v1.0.0 h1:validhash=
github.com/baz/qux v1.0.0 sha256:notavalidformat
`)
	hashes, err := ParseGoSum(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should only have the h1: hash
	if len(hashes) != 1 {
		t.Fatalf("expected 1 hash (h1: only), got %d", len(hashes))
	}
}

// MergeHashes tests

func TestMergeHashes_Basic(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/foo/bar", Version: "v1.0.0"},
		{ImportPath: "github.com/baz/qux", Version: "v2.0.0"},
	}
	hashes := map[string]string{
		"github.com/foo/bar v1.0.0": "h1:foohash=",
		"github.com/baz/qux v2.0.0": "h1:bazhash=",
	}

	MergeHashes(deps, hashes)

	if deps[0].GoSumHash != "h1:foohash=" {
		t.Errorf("expected h1:foohash=, got %s", deps[0].GoSumHash)
	}
	if deps[1].GoSumHash != "h1:bazhash=" {
		t.Errorf("expected h1:bazhash=, got %s", deps[1].GoSumHash)
	}
}

func TestMergeHashes_MissingHash(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/foo/bar", Version: "v1.0.0"},
		{ImportPath: "github.com/missing/hash", Version: "v2.0.0"},
	}
	hashes := map[string]string{
		"github.com/foo/bar v1.0.0": "h1:foohash=",
		// No hash for missing/hash
	}

	MergeHashes(deps, hashes)

	if deps[0].GoSumHash != "h1:foohash=" {
		t.Errorf("expected h1:foohash=, got %s", deps[0].GoSumHash)
	}
	if deps[1].GoSumHash != "" {
		t.Errorf("expected empty hash for missing, got %s", deps[1].GoSumHash)
	}
}

func TestMergeHashes_EmptyHashes(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/foo/bar", Version: "v1.0.0"},
	}
	hashes := map[string]string{}

	MergeHashes(deps, hashes)

	if deps[0].GoSumHash != "" {
		t.Errorf("expected empty hash, got %s", deps[0].GoSumHash)
	}
}

// Replace directive tests

func TestParseReplaces_LocalPath(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/foo/bar v1.0.0

replace github.com/foo/bar => ../local/bar
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 1 {
		t.Fatalf("expected 1 replace, got %d", len(replaces))
	}
	if replaces[0].Old != "github.com/foo/bar" {
		t.Errorf("expected Old=github.com/foo/bar, got %s", replaces[0].Old)
	}
	if replaces[0].NewPath != "../local/bar" {
		t.Errorf("expected NewPath=../local/bar, got %s", replaces[0].NewPath)
	}
	if !replaces[0].IsLocal() {
		t.Error("expected IsLocal()=true for relative path")
	}
}

func TestParseReplaces_LocalPathAbsolute(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

replace github.com/foo/bar => /absolute/path/bar
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 1 {
		t.Fatalf("expected 1 replace, got %d", len(replaces))
	}
	if !replaces[0].IsLocal() {
		t.Error("expected IsLocal()=true for absolute path")
	}
}

func TestParseReplaces_RemoteReplace(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

replace github.com/original/pkg => github.com/fork/pkg v1.2.3
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 1 {
		t.Fatalf("expected 1 replace, got %d", len(replaces))
	}
	if replaces[0].Old != "github.com/original/pkg" {
		t.Errorf("expected Old=github.com/original/pkg, got %s", replaces[0].Old)
	}
	if replaces[0].NewPath != "github.com/fork/pkg" {
		t.Errorf("expected NewPath=github.com/fork/pkg, got %s", replaces[0].NewPath)
	}
	if replaces[0].NewVersion != "v1.2.3" {
		t.Errorf("expected NewVersion=v1.2.3, got %s", replaces[0].NewVersion)
	}
	if replaces[0].IsLocal() {
		t.Error("expected IsLocal()=false for remote replace")
	}
}

func TestParseReplaces_VersionSpecific(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

replace github.com/foo/bar v1.0.0 => ../local/bar
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 1 {
		t.Fatalf("expected 1 replace, got %d", len(replaces))
	}
	if replaces[0].OldVersion != "v1.0.0" {
		t.Errorf("expected OldVersion=v1.0.0, got %s", replaces[0].OldVersion)
	}
}

func TestParseReplaces_MultipleReplaces(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

replace (
	github.com/foo/bar => ../local/bar
	github.com/baz/qux => ./qux
	github.com/remote/pkg => github.com/fork/pkg v1.0.0
)
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 3 {
		t.Fatalf("expected 3 replaces, got %d", len(replaces))
	}
}

func TestParseReplaces_NoReplaces(t *testing.T) {
	input := []byte(`module example.com/mymod

go 1.21

require github.com/foo/bar v1.0.0
`)
	replaces, err := ParseReplaces(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(replaces) != 0 {
		t.Errorf("expected 0 replaces, got %d", len(replaces))
	}
}

func TestFilterLocalReplaces(t *testing.T) {
	replaces := []Replace{
		{Old: "github.com/foo/bar", NewPath: "../local/bar"},
		{Old: "github.com/baz/qux", NewPath: "github.com/fork/qux", NewVersion: "v1.0.0"},
		{Old: "github.com/another/pkg", NewPath: "./pkg"},
	}

	local := FilterLocalReplaces(replaces)
	if len(local) != 2 {
		t.Fatalf("expected 2 local replaces, got %d", len(local))
	}
	if local[0].Old != "github.com/foo/bar" {
		t.Errorf("expected first local replace to be foo/bar, got %s", local[0].Old)
	}
	if local[1].Old != "github.com/another/pkg" {
		t.Errorf("expected second local replace to be another/pkg, got %s", local[1].Old)
	}
}

func TestFilterExternalReplaces(t *testing.T) {
	replaces := []Replace{
		{Old: "github.com/foo/bar", NewPath: "../local/bar"},
		{Old: "github.com/baz/qux", NewPath: "github.com/fork/qux", NewVersion: "v1.0.0"},
		{Old: "github.com/another/pkg", NewPath: "./pkg"},
		{Old: "github.com/remote/dep", NewPath: "github.com/myfork/dep", NewVersion: "v2.0.0"},
	}

	external := FilterExternalReplaces(replaces)
	if len(external) != 2 {
		t.Fatalf("expected 2 external replaces, got %d", len(external))
	}
	if external[0].Old != "github.com/baz/qux" {
		t.Errorf("expected first external replace to be baz/qux, got %s", external[0].Old)
	}
	if external[1].Old != "github.com/remote/dep" {
		t.Errorf("expected second external replace to be remote/dep, got %s", external[1].Old)
	}
}

func TestReplace_IsExternal(t *testing.T) {
	tests := []struct {
		name     string
		replace  Replace
		expected bool
	}{
		{
			name:     "relative path",
			replace:  Replace{Old: "pkg", NewPath: "../local"},
			expected: false,
		},
		{
			name:     "absolute path",
			replace:  Replace{Old: "pkg", NewPath: "/absolute/path"},
			expected: false,
		},
		{
			name:     "current dir path",
			replace:  Replace{Old: "pkg", NewPath: "./local"},
			expected: false,
		},
		{
			name:     "external module",
			replace:  Replace{Old: "pkg", NewPath: "github.com/fork/pkg", NewVersion: "v1.0.0"},
			expected: true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if tc.replace.IsExternal() != tc.expected {
				t.Errorf("IsExternal() = %v, want %v", tc.replace.IsExternal(), tc.expected)
			}
		})
	}
}

func TestApplyExternalReplaces_Basic(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/original/pkg", Version: "v1.0.0"},
		{ImportPath: "github.com/normal/dep", Version: "v2.0.0"},
	}

	replaces := []Replace{
		{Old: "github.com/original/pkg", NewPath: "github.com/fork/pkg", NewVersion: "v1.0.0"},
		{Old: "github.com/local/thing", NewPath: "../local"}, // local, should be ignored
	}

	ApplyExternalReplaces(deps, replaces)

	// original/pkg should have FetchPath set
	if deps[0].FetchPath != "github.com/fork/pkg" {
		t.Errorf("expected FetchPath=github.com/fork/pkg, got %s", deps[0].FetchPath)
	}

	// normal/dep should have no FetchPath
	if deps[1].FetchPath != "" {
		t.Errorf("expected empty FetchPath for non-replaced dep, got %s", deps[1].FetchPath)
	}
}

func TestApplyExternalReplaces_VersionSpecific(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/pkg", Version: "v1.0.0"},
		{ImportPath: "github.com/pkg", Version: "v2.0.0"},
	}

	replaces := []Replace{
		// Only replace v1.0.0
		{Old: "github.com/pkg", OldVersion: "v1.0.0", NewPath: "github.com/fork/pkg", NewVersion: "v1.0.0"},
	}

	ApplyExternalReplaces(deps, replaces)

	// v1.0.0 should be replaced
	if deps[0].FetchPath != "github.com/fork/pkg" {
		t.Errorf("expected FetchPath for v1.0.0, got %s", deps[0].FetchPath)
	}

	// v2.0.0 should NOT be replaced (version-specific replace)
	if deps[1].FetchPath != "" {
		t.Errorf("expected no FetchPath for v2.0.0, got %s", deps[1].FetchPath)
	}
}

func TestApplyExternalReplaces_VersionUpdate(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/original/pkg", Version: "v1.0.0"},
	}

	replaces := []Replace{
		// Replace with different version
		{Old: "github.com/original/pkg", NewPath: "github.com/fork/pkg", NewVersion: "v2.0.0"},
	}

	ApplyExternalReplaces(deps, replaces)

	if deps[0].FetchPath != "github.com/fork/pkg" {
		t.Errorf("expected FetchPath=github.com/fork/pkg, got %s", deps[0].FetchPath)
	}
	if deps[0].Version != "v2.0.0" {
		t.Errorf("expected Version=v2.0.0, got %s", deps[0].Version)
	}
}

func TestApplyExternalReplaces_ModuleWideOverVersionSpecific(t *testing.T) {
	deps := []Dependency{
		{ImportPath: "github.com/pkg", Version: "v1.5.0"},
	}

	replaces := []Replace{
		// Version-specific replace for v1.0.0
		{Old: "github.com/pkg", OldVersion: "v1.0.0", NewPath: "github.com/fork1/pkg", NewVersion: "v1.0.0"},
		// Module-wide replace (no version)
		{Old: "github.com/pkg", NewPath: "github.com/fork2/pkg", NewVersion: "v2.0.0"},
	}

	ApplyExternalReplaces(deps, replaces)

	// v1.5.0 should use module-wide replace (fork2), not version-specific (fork1)
	if deps[0].FetchPath != "github.com/fork2/pkg" {
		t.Errorf("expected FetchPath=github.com/fork2/pkg (module-wide), got %s", deps[0].FetchPath)
	}
}

func TestDependency_EffectiveFetchPath(t *testing.T) {
	tests := []struct {
		name     string
		dep      Dependency
		expected string
	}{
		{
			name:     "no FetchPath",
			dep:      Dependency{ImportPath: "github.com/foo/bar"},
			expected: "github.com/foo/bar",
		},
		{
			name:     "with FetchPath",
			dep:      Dependency{ImportPath: "github.com/original/pkg", FetchPath: "github.com/fork/pkg"},
			expected: "github.com/fork/pkg",
		},
		{
			name:     "empty FetchPath",
			dep:      Dependency{ImportPath: "github.com/foo/bar", FetchPath: ""},
			expected: "github.com/foo/bar",
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			result := tc.dep.EffectiveFetchPath()
			if result != tc.expected {
				t.Errorf("EffectiveFetchPath() = %q, want %q", result, tc.expected)
			}
		})
	}
}
