package goparse

import (
	"go/build/constraint"
	"os"
	"path/filepath"
	"reflect"
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

func TestBuildContextMatches(t *testing.T) {
	linuxAmd64 := BuildContext{GOOS: "linux", GOARCH: "amd64", CgoEnabled: true, GoVersion: "1.24"}
	darwinArm64 := BuildContext{GOOS: "darwin", GOARCH: "arm64", CgoEnabled: true, GoVersion: "1.24"}
	windows := BuildContext{GOOS: "windows", GOARCH: "amd64", GoVersion: "1.24"}
	tagged := BuildContext{GOOS: "linux", GOARCH: "amd64", GoVersion: "1.24", Tags: []string{"integration"}}
	old := BuildContext{GOOS: "linux", GOARCH: "amd64", GoVersion: "1.20"}

	build := func(expr string) constraint.Expr {
		e, err := constraint.Parse("//go:build " + expr)
		if err != nil {
			t.Fatal(err)
		}
		return e
	}

	tests := []struct {
		name    string
		file    *GoFile
		ctx     BuildContext
		matches bool
	}{
		{"plain", &GoFile{Path: "foo.go"}, linuxAmd64, true},
		{"_linux on linux", &GoFile{Path: "foo_linux.go"}, linuxAmd64, true},
		{"_linux on darwin", &GoFile{Path: "foo_linux.go"}, darwinArm64, false},
		{"_amd64", &GoFile{Path: "foo_amd64.go"}, linuxAmd64, true},
		{"_windows_amd64 on linux", &GoFile{Path: "foo_windows_amd64.go"}, linuxAmd64, false},
		{"unix on linux", &GoFile{Path: "u.go", Constraint: build("unix")}, linuxAmd64, true},
		{"unix on macos", &GoFile{Path: "u.go", Constraint: build("unix")}, darwinArm64, true},
		{"unix on windows", &GoFile{Path: "u.go", Constraint: build("unix")}, windows, false},
		{"go1.21 with 1.24", &GoFile{Path: "v.go", Constraint: build("go1.21")}, linuxAmd64, true},
		{"go1.21 with 1.20", &GoFile{Path: "v.go", Constraint: build("go1.21")}, old, false},
		{"!go1.21 with 1.24", &GoFile{Path: "v.go", Constraint: build("!go1.21")}, linuxAmd64, false},
		{"custom tag unset", &GoFile{Path: "i.go", Constraint: build("integration")}, linuxAmd64, false},
		{"custom tag set", &GoFile{Path: "i.go", Constraint: build("integration")}, tagged, true},
		{"cgo on", &GoFile{Path: "c.go", Constraint: build("cgo")}, linuxAmd64, true},
		{"cgo off", &GoFile{Path: "c.go", Constraint: build("cgo")}, windows, false},
		{"import C without cgo", &GoFile{Path: "c.go", HasCgo: true}, windows, false},
		{"gc", &GoFile{Path: "g.go", Constraint: build("gc && !gccgo")}, linuxAmd64, true},
	}

	for _, tt := range tests {
		if got := tt.ctx.Matches(tt.file); got != tt.matches {
			t.Errorf("%s: Matches = %v; want %v", tt.name, got, tt.matches)
		}
	}
}

func TestParseFileEmbedPatterns(t *testing.T) {
	content := "package testpkg\n" +
		"import \"embed\"\n" +
		"//go:embed static/*.html \"my file.txt\" `raw dir`\n" +
		"var files embed.FS\n" +
		"//go:embed\tversion.txt\n" +
		"var version string\n" +
		"// see //go:embed notadirective\n" +
		"var s = \"//go:embed instring\"\n" +
		"/* //go:embed inblock */\n"
	path := filepath.Join(t.TempDir(), "embed.go")
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	gf, err := ParseFile(path)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"static/*.html", "my file.txt", "raw dir", "version.txt"}
	if !reflect.DeepEqual(gf.EmbedDirs, want) {
		t.Errorf("EmbedDirs = %q, want %q", gf.EmbedDirs, want)
	}
}

func TestParseFileRejectsAMalformedEmbed(t *testing.T) {
	content := "package testpkg\n//go:embed \"unterminated\nvar b []byte\n"
	path := filepath.Join(t.TempDir(), "embed.go")
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := ParseFile(path); err == nil {
		t.Error("ParseFile accepted an unterminated //go:embed pattern")
	}
}
