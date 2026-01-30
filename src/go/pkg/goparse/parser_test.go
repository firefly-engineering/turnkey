package goparse

import (
	"os"
	"path/filepath"
	"testing"
)

func TestParseFile(t *testing.T) {
	content := `//go:build linux && amd64
package testpkg
import (
	"fmt"
	"os"
	"C"
)
//go:embed testdata/*
var data []byte
`
	tmpdir := t.TempDir()
	path := filepath.Join(tmpdir, "test.go")
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	gf, err := ParseFile(path)
	if err != nil {
		t.Fatal(err)
	}

	if gf.Package != "testpkg" {
		t.Errorf("expected package testpkg, got %s", gf.Package)
	}

	expectedImports := map[string]bool{"fmt": true, "os": true, "C": true}
	for _, imp := range gf.Imports {
		delete(expectedImports, imp)
	}
	if len(expectedImports) > 0 {
		t.Errorf("missing imports: %v", expectedImports)
	}

	if !gf.HasCgo {
		t.Error("expected HasCgo to be true")
	}

	if gf.Constraint == nil {
		t.Error("expected build constraint to be parsed")
	}

	if len(gf.EmbedDirs) != 1 || gf.EmbedDirs[0] != "testdata/*" {
		t.Errorf("expected embed testdata/*, got %v", gf.EmbedDirs)
	}
}

func TestParseFilenameConstraint(t *testing.T) {
	tests := []struct {
		filename string
		os       string
		arch     string
	}{
		{"foo.go", "", ""},
		{"foo_linux.go", "linux", ""},
		{"foo_amd64.go", "", "amd64"},
		{"foo_linux_amd64.go", "linux", "amd64"},
		{"foo_test.go", "", ""},
		{"foo_linux_test.go", "linux", ""},
	}

	for _, tt := range tests {
		os, arch := ParseFilenameConstraint(tt.filename)
		if os != tt.os || arch != tt.arch {
			t.Errorf("ParseFilenameConstraint(%s) = (%s, %s); want (%s, %s)", tt.filename, os, arch, tt.os, tt.arch)
		}
	}
}

func TestMatchesPlatform(t *testing.T) {
	linuxAmd64 := Platform{OS: "linux", Arch: "amd64"}
	darwinArm64 := Platform{OS: "darwin", Arch: "arm64"}

	tests := []struct {
		file     *GoFile
		platform Platform
		matches  bool
	}{
		{
			file:     &GoFile{Path: "foo.go"},
			platform: linuxAmd64,
			matches:  true,
		},
		{
			file:     &GoFile{Path: "foo_linux.go"},
			platform: linuxAmd64,
			matches:  true,
		},
		{
			file:     &GoFile{Path: "foo_linux.go"},
			platform: darwinArm64,
			matches:  false,
		},
		{
			file:     &GoFile{Path: "foo_amd64.go"},
			platform: linuxAmd64,
			matches:  true,
		},
		{
			file:     &GoFile{Path: "foo_windows_amd64.go"},
			platform: linuxAmd64,
			matches:  false,
		},
	}

	for _, tt := range tests {
		if got := MatchesPlatform(tt.file, tt.platform); got != tt.matches {
			t.Errorf("MatchesPlatform(%s, %v) = %v; want %v", tt.file.Path, tt.platform, got, tt.matches)
		}
	}
}
