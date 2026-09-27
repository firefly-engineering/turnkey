package goparse

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// A configuration sets the tags whose dimension it sets on, and no other.
func TestConfigTags(t *testing.T) {
	config := conditions.Configuration{
		conditions.OS:            "linux",
		TagDimension("b").Name:   conditions.Set,
		TagDimension("a").Name:   conditions.Set,
		TagDimension("off").Name: conditions.Unset,
	}
	if got, want := ConfigTags(config), []string{"a", "b"}; !reflect.DeepEqual(got, want) {
		t.Errorf("ConfigTags = %v, want %v", got, want)
	}
}

// A tag's dimension is keyed on the prelude's constraint for it.
func TestTagDimensionKeys(t *testing.T) {
	space := conditions.NewSpace(nil, "").WithDimensions([]conditions.OnOff{TagDimension("integration")})
	split := space.Split(func(c conditions.Configuration) []string {
		return ConfigTags(c)
	})
	want := []conditions.Branch{
		{Key: "prelude//go/tags/constraints:integration[set]", Labels: []string{"integration"}},
		{Key: "prelude//go/tags/constraints:integration[unset]"},
	}
	if !reflect.DeepEqual(split.Branches, want) {
		t.Errorf("branches = %+v, want %+v", split.Branches, want)
	}
}

// A configuration's platform, in Buck2's names, is its Go build's GOOS and
// GOARCH, with cgo on and the tags it sets.
func TestConfigContext(t *testing.T) {
	cases := []struct {
		os, cpu      string
		goos, goarch string
	}{
		{"linux", "x86_64", "linux", "amd64"},
		{"linux", "arm64", "linux", "arm64"},
		{"macos", "x86_64", "darwin", "amd64"},
		{"macos", "arm64", "darwin", "arm64"},
	}
	for _, c := range cases {
		config := conditions.Configuration{
			conditions.OS:                    c.os,
			conditions.CPU:                   c.cpu,
			TagDimension("integration").Name: conditions.Set,
		}
		ctx, ok := ConfigContext(config)
		want := BuildContext{GOOS: c.goos, GOARCH: c.goarch, CgoEnabled: true, Tags: []string{"integration"}}
		if !ok || !reflect.DeepEqual(ctx, want) {
			t.Errorf("ConfigContext(%s) = %+v, %v; want %+v, true", config, ctx, ok, want)
		}
		wantEnv := []string{"GOOS=" + c.goos, "GOARCH=" + c.goarch, "CGO_ENABLED=1"}
		if got := ctx.Environ(); !reflect.DeepEqual(got, wantEnv) {
			t.Errorf("Environ(%s) = %v, want %v", config, got, wantEnv)
		}
	}
}

// Without a platform Go has names for, a configuration's Go build has no
// GOOS or GOARCH, and the go command gets the host's.
func TestConfigContextWithoutPlatform(t *testing.T) {
	for _, config := range []conditions.Configuration{
		{},
		{conditions.OS: "windows", conditions.CPU: "x86_64"},
		{conditions.OS: "linux", conditions.CPU: "riscv64"},
	} {
		ctx, ok := ConfigContext(config)
		if ok || ctx.GOOS != "" || ctx.GOARCH != "" {
			t.Errorf("ConfigContext(%s) = %+v, %v; want no platform", config, ctx, ok)
		}
		if env := ctx.Environ(); env != nil {
			t.Errorf("Environ(%s) = %v, want none", config, env)
		}
	}
}

// A tree's constraint tags are its Go files' (test files and subpackages
// included, go:build or +build), but not those of the directories go
// list's ./... skips.
func TestTreeConstraintTags(t *testing.T) {
	dir := t.TempDir()
	files := map[string]string{
		"a.go":             "//go:build linux && !integration\n\npackage a\n",
		"a_test.go":        "//go:build e2e\n\npackage a\n",
		"old.go":           "// +build legacy\n\npackage a\n",
		"plain.go":         "package a\n\n// +build ignored_after_package\n",
		"sub/b.go":         "//go:build (cgo || netgo) && !arm64\n\npackage b\n",
		"testdata/c.go":    "//go:build skipped_testdata\n\npackage c\n",
		"vendor/v/v.go":    "//go:build skipped_vendor\n\npackage v\n",
		".hidden/h.go":     "//go:build skipped_hidden\n\npackage h\n",
		"_underscore/u.go": "//go:build skipped_underscore\n\npackage u\n",
		"notgo.txt":        "//go:build skipped_notgo\n",
	}
	for name, content := range files {
		path := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	got, err := TreeConstraintTags(dir)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"arm64", "cgo", "e2e", "integration", "legacy", "linux", "netgo"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("TreeConstraintTags = %v, want %v", got, want)
	}
}
