package buckgen

import (
	"bytes"
	"go/build/constraint"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
)

// testConfig is buckgen's configuration for turnkey's default platforms.
func testConfig() *Config {
	return &Config{
		Buck: BuckConfig{
			GoLibraryRule:         "go_library",
			DepsTargetLabelPrefix: "godeps//",
			DepsAttr:              "deps",
			BuildfileName:         "rules.star",
		},
		Conditions: ConditionsConfig{
			Settings: "toolchains//conditions",
			Platforms: []conditions.Platform{
				{OS: "linux", CPU: "x86_64"},
				{OS: "linux", CPU: "arm64"},
				{OS: "macos", CPU: "x86_64"},
				{OS: "macos", CPU: "arm64"},
			},
			GoTags: []string{"integration"},
		},
		GoVersion: "1.24",
	}
}

func file(t *testing.T, path, build string, imports ...string) *goparse.GoFile {
	t.Helper()
	f := &goparse.GoFile{Path: path, Imports: imports}
	if build != "" {
		expr, err := constraint.Parse("//go:build " + build)
		if err != nil {
			t.Fatal(err)
		}
		f.Constraint = expr
	}
	return f
}

func render(t *testing.T, pkg *goparse.GoPackage, cfg *Config) string {
	t.Helper()
	var buf bytes.Buffer
	built, err := RenderPackage(&buf, pkg, cfg)
	if err != nil {
		t.Fatalf("RenderPackage failed: %v", err)
	}
	if !built {
		t.Fatal("package not built on any platform")
	}
	return buf.String()
}

func TestRenderPackage(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/testpkg",
		Files: []*goparse.GoFile{
			file(t, "common.go", "", "fmt", "github.com/example/common"),
			file(t, "x_linux.go", "", "github.com/example/linuxonly"),
			file(t, "x_darwin.go", "", "github.com/example/maconly"),
		},
	}
	want := `go_library(
    name = "testpkg",
    package_name = "github.com/example/testpkg",
    srcs = native.glob(["*.go", "*.s", "*.h", "*.c", "*.cc", "*.cpp", "*.S"]),
    header_namespace = "",
    visibility = ["PUBLIC"],
    deps = ["godeps//github.com/example/common:common"] + select({
        "config//os:linux": ["godeps//github.com/example/linuxonly:linuxonly"],
        "config//os:macos": ["godeps//github.com/example/maconly:maconly"],
    }),
)
`
	if got := render(t, pkg, testConfig()); got != want {
		t.Errorf("output:\n%s\nwant:\n%s", got, want)
	}
}

// A //go:build unix file is included on Linux and macOS, and a go1.21 file
// with a newer toolchain: their imports are common.
func TestRenderPackageUnixAndReleaseTags(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/sys",
		Files: []*goparse.GoFile{
			file(t, "unix.go", "unix", "github.com/example/unixdep"),
			file(t, "new.go", "go1.21", "github.com/example/newdep"),
			file(t, "future.go", "go1.99", "github.com/example/futuredep"),
		},
	}
	got := render(t, pkg, testConfig())
	want := `    deps = [
        "godeps//github.com/example/newdep:newdep",
        "godeps//github.com/example/unixdep:unixdep",
    ],
`
	if !strings.Contains(got, want) {
		t.Errorf("output:\n%s\nwant deps:\n%s", got, want)
	}
}

// A linux/arm64-only import is keyed on the combined OS-and-CPU setting,
// not on a duplicated OS key, and there is no DEFAULT.
func TestRenderPackageLinuxArm64Only(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/cpu",
		Files: []*goparse.GoFile{
			file(t, "cpu.go", ""),
			file(t, "cpu_linux_arm64.go", "", "github.com/example/armdep"),
		},
	}
	got := render(t, pkg, testConfig())
	want := `    deps = select({
        "toolchains//conditions:linux-arm64": ["godeps//github.com/example/armdep:armdep"],
        "toolchains//conditions:linux-x86_64": [],
        "toolchains//conditions:macos-arm64": [],
        "toolchains//conditions:macos-x86_64": [],
    }),
`
	if !strings.Contains(got, want) || strings.Contains(got, "DEFAULT") {
		t.Errorf("output:\n%s\nwant deps:\n%s", got, want)
	}
}

// An allowed tag is a dimension; a tag that isn't allowed is never set.
func TestRenderPackageTags(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/tagged",
		Files: []*goparse.GoFile{
			file(t, "it.go", "integration", "github.com/example/harness"),
			file(t, "other.go", "othertag", "github.com/example/never"),
			file(t, "base.go", ""),
		},
	}
	got := render(t, pkg, testConfig())
	want := `    deps = select({
        "prelude//go/tags/constraints:integration[set]": ["godeps//github.com/example/harness:harness"],
        "prelude//go/tags/constraints:integration[unset]": [],
    }),
`
	if !strings.Contains(got, want) {
		t.Errorf("output:\n%s\nwant deps:\n%s", got, want)
	}
}

// Go lets a package import its parent (cobra/doc imports cobra): that's a
// dep, not a self-reference.
func TestRenderPackageImportsParent(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/parent/child",
		Files: []*goparse.GoFile{
			file(t, "child.go", "", "github.com/example/parent", "github.com/example/parent/child"),
		},
	}
	got := render(t, pkg, testConfig())
	want := `    deps = ["godeps//github.com/example/parent:parent"],
`
	if !strings.Contains(got, want) {
		t.Errorf("output:\n%s\nwant deps:\n%s", got, want)
	}
}

func TestRenderPackageNoDeps(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/nodeps",
		Files:      []*goparse.GoFile{file(t, "a.go", "", "fmt", "os")},
	}
	got := render(t, pkg, testConfig())
	if strings.Contains(got, "deps =") {
		t.Errorf("output has a deps attribute:\n%s", got)
	}
}

// A package no platform builds gets no rules.star.
func TestRenderPackageNotBuilt(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/win",
		Files:      []*goparse.GoFile{file(t, "w_windows.go", "")},
	}
	var buf bytes.Buffer
	built, err := RenderPackage(&buf, pkg, testConfig())
	if err != nil {
		t.Fatal(err)
	}
	if built || buf.Len() != 0 {
		t.Errorf("windows-only package rendered:\n%s", buf.String())
	}
}

func writeFiles(t *testing.T, dir string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		path := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func TestRenderModule(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{
		"go.mod":              "module example.com/mod\n",
		"mod.go":              "package mod\n\nimport (\n\t\"fmt\"\n\t\"example.com/other/x\"\n)\n",
		"sub/sub.go":          "package sub\n\nimport \"example.com/mod\"\nimport \"example.com/other/y\"\n",
		"win/win.go":          "//go:build windows\n\npackage win\n\nimport \"example.com/never\"\n",
		"docs/README.md":      "not a package\n",
		"testdata/t/t.go":     "package t\n\nimport \"example.com/testonly\"\n",
		"_examples/e/e.go":    "package e\n",
		"internal/deep/d.go":  "package deep\n\nimport \"example.com/other/x\"\n",
		"sub/sub_test.go":     "package sub\n\nimport \"example.com/testdep\"\n",
		".hidden/h/h.go":      "package h\n",
		"internal/deep/d.s":   "#include \"textflag.h\"\n",
		"internal/deep/doc.h": "\n",
	})

	rendered, imports, err := RenderModule(dir, "example.com/mod", testConfig())
	if err != nil {
		t.Fatal(err)
	}
	wantRendered := []RenderedPackage{
		{Subdir: ".", Target: "mod"},
		{Subdir: "internal/deep", Target: "deep"},
		{Subdir: "sub", Target: "sub"},
	}
	if !slices.Equal(rendered, wantRendered) {
		t.Errorf("rendered = %v, want %v", rendered, wantRendered)
	}
	// sub's import of its parent is a dep; test-only and unbuilt packages
	// reference nothing
	wantImports := []string{"example.com/mod", "example.com/other/x", "example.com/other/y"}
	if !slices.Equal(imports, wantImports) {
		t.Errorf("imports = %v, want %v", imports, wantImports)
	}

	sub, err := os.ReadFile(filepath.Join(dir, "sub", "rules.star"))
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{
		`package_name = "example.com/mod/sub"`,
		`"godeps//example.com/mod:mod"`,
		`"godeps//example.com/other/y:y"`,
	} {
		if !strings.Contains(string(sub), want) {
			t.Errorf("sub/rules.star missing %s:\n%s", want, sub)
		}
	}
	for _, unrendered := range []string{"win", "docs", "testdata/t", "_examples/e", ".hidden/h"} {
		if _, err := os.Stat(filepath.Join(dir, unrendered, "rules.star")); err == nil {
			t.Errorf("%s rendered", unrendered)
		}
	}
}
