package buckgen

import (
	"bytes"
	"go/build/constraint"
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

func TestRenderPackageWithLocalReplaces(t *testing.T) {
	pkg := &goparse.GoPackage{
		ImportPath: "github.com/example/testpkg",
		Files: []*goparse.GoFile{file(t, "a.go", "",
			"fmt",
			"github.com/company/shared-lib",        // locally replaced
			"github.com/company/shared-lib/subpkg", // subpkg of locally replaced
			"github.com/external/dep",              // not replaced
		)},
	}
	cfg := testConfig()
	cfg.LocalReplaces = map[string]string{
		"github.com/company/shared-lib": "//src/shared-lib:shared-lib",
	}
	output := render(t, pkg, cfg)
	for _, want := range []string{
		"\"//src/shared-lib:shared-lib\"",
		"\"//src/shared-lib/subpkg:subpkg\"",
		"\"godeps//github.com/external/dep:dep\"",
	} {
		if !strings.Contains(output, want) {
			t.Errorf("output missing %s:\n%s", want, output)
		}
	}
}

func TestImportToTargetWithLocalReplace(t *testing.T) {
	cfg := testConfig()
	cfg.LocalReplaces = map[string]string{
		"github.com/company/mylib": "//libs/mylib:mylib",
	}

	tests := []struct {
		importPath string
		expected   string
	}{
		// Direct match
		{"github.com/company/mylib", "//libs/mylib:mylib"},
		// Subpackage of replaced module
		{"github.com/company/mylib/subpkg", "//libs/mylib/subpkg:subpkg"},
		{"github.com/company/mylib/deep/nested", "//libs/mylib/deep/nested:nested"},
		// Non-replaced import
		{"github.com/external/pkg", "godeps//github.com/external/pkg:pkg"},
	}

	for _, tc := range tests {
		t.Run(tc.importPath, func(t *testing.T) {
			result := importToTarget(tc.importPath, cfg)
			if result != tc.expected {
				t.Errorf("importToTarget(%q) = %q, want %q", tc.importPath, result, tc.expected)
			}
		})
	}
}
