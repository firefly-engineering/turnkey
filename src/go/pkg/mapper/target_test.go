package mapper

import (
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// fakeLanguage is a plug-in that resolves every package to mapping, and on
// Linux adds //linux:only to its deps. A target's "features" variant adds
// //feature/<name>:<name> per feature. It records what it resolved.
type fakeLanguage struct {
	mapping  PackageMapping
	requests []Request
}

func (l *fakeLanguage) Name() string { return "fake" }

func (l *fakeLanguage) RuleKind(rule string) (TargetKind, bool) {
	kind, ok := map[string]TargetKind{"fake_library": Library, "fake_test": Test}[rule]
	return kind, ok
}

func (l *fakeLanguage) DepsAttribute() string    { return "deps" }
func (l *fakeLanguage) SourcePatterns() []string { return []string{"*.fake"} }

func (l *fakeLanguage) Dimensions(string) ([]string, error) {
	return []string{conditions.OS}, nil
}

func (l *fakeLanguage) VariantAttributes(TargetKind) []string { return []string{"features"} }

func (l *fakeLanguage) ResolveDeps(_ string, req Request) (PackageMapping, error) {
	l.requests = append(l.requests, req)
	m := l.mapping
	m.Deps = append([]MappedDep(nil), m.Deps...)
	if req.Config[conditions.OS] == "linux" {
		m.Deps = append(m.Deps, MappedDep{Target: "//linux:only"})
	}
	if features, ok := starlark.Labels(req.Variant["features"]); ok {
		for _, f := range features {
			m.Deps = append(m.Deps, MappedDep{Target: "//feature/" + f + ":" + f})
		}
	}
	return m, nil
}

var (
	fakePlatforms = []conditions.Platform{
		{OS: "linux", CPU: "x86_64"},
		{OS: "linux", CPU: "arm64"},
		{OS: "macos", CPU: "arm64"},
	}
	onLinux = conditions.Configuration{conditions.OS: "linux", conditions.CPU: "x86_64"}
	onMacOS = conditions.Configuration{conditions.OS: "macos", conditions.CPU: "arm64"}
)

// openFake opens the package /repo/pkg with lang, over fakePlatforms.
func openFake(t *testing.T, lang *fakeLanguage) *Package {
	t.Helper()
	pkg, err := OpenPackage(lang, "/repo", "/repo/pkg", conditions.NewSpace(fakePlatforms, ""))
	if err != nil {
		t.Fatal(err)
	}
	return pkg
}

// fakeTarget parses a rules.star source's only target and returns what it
// wants.
func fakeTarget(t *testing.T, pkg *Package, src string) *Target {
	t.Helper()
	f, err := starlark.Parse("rules.star", []byte(src))
	if err != nil {
		t.Fatal(err)
	}
	kind, _ := pkg.lang.RuleKind(f.Targets[0].Rule)
	target, attr, ok := pkg.Target(f.Targets[0], kind)
	if !ok {
		t.Fatalf("attribute %s unreadable", attr)
	}
	return target
}

func wantDeps(t *testing.T, target *Target, config conditions.Configuration, old []string) Want {
	t.Helper()
	w, err := target.Deps(config, old)
	if err != nil {
		t.Fatal(err)
	}
	return w
}

// A library wants its package's deps in each configuration, and the
// package is resolved once per value of the dimensions the language names,
// however many configurations share it.
func TestTargetWantsDepsPerConfiguration(t *testing.T) {
	lang := &fakeLanguage{mapping: PackageMapping{Deps: []MappedDep{{Target: "//unix:unix"}}}}
	pkg := openFake(t, lang)
	lib := fakeTarget(t, pkg, `fake_library(name = "lib")`)

	if got, want := wantDeps(t, lib, onLinux, nil).Labels, []string{"//unix:unix", "//linux:only"}; !reflect.DeepEqual(got, want) {
		t.Errorf("Linux labels = %v, want %v", got, want)
	}
	if got, want := wantDeps(t, lib, onMacOS, nil).Labels, []string{"//unix:unix"}; !reflect.DeepEqual(got, want) {
		t.Errorf("macOS labels = %v, want %v", got, want)
	}
	for _, config := range pkg.Space().Configurations {
		wantDeps(t, lib, config, nil)
	}
	if len(lang.requests) != 2 {
		t.Errorf("resolved %d times, want once per OS: %+v", len(lang.requests), lang.requests)
	}
	for _, req := range lang.requests {
		if _, ok := req.Config[conditions.CPU]; ok {
			t.Errorf("request config %v has the CPU, which the language doesn't depend on", req.Config)
		}
	}
}

// A target's variant is read in each configuration, a select() included,
// and resolved with.
func TestTargetResolvesItsVariant(t *testing.T) {
	pkg := openFake(t, &fakeLanguage{})
	lib := fakeTarget(t, pkg, `fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
)`)
	if got, want := wantDeps(t, lib, onMacOS, nil).Labels, []string{"//feature/a:a", "//feature/b:b"}; !reflect.DeepEqual(got, want) {
		t.Errorf("macOS labels = %v, want %v", got, want)
	}
}

// A variant attribute whose select() has a key sync doesn't know makes
// the target unreadable.
func TestTargetUnreadableVariant(t *testing.T) {
	pkg := openFake(t, &fakeLanguage{})
	f, err := starlark.Parse("rules.star", []byte(`fake_library(name = "lib", features = select({"//my:setting": ["a"]}))`))
	if err != nil {
		t.Fatal(err)
	}
	if _, attr, ok := pkg.Target(f.Targets[0], Library); ok || attr != "features" {
		t.Errorf("Target = %q, %v; want features unreadable", attr, ok)
	}
}

// A test wants its test-only deps, and its library's deps unless it gets
// them through a target_under_test or a same-package dep it has. Test-only
// imports that couldn't be mapped leave only the test incomplete.
func TestTestTargetComposition(t *testing.T) {
	mapping := PackageMapping{
		Deps:                []MappedDep{{Target: "//lib:lib"}},
		TestDeps:            []MappedDep{{Target: "//check:check"}, {Target: "//lib:lib"}},
		UnmappedImports:     []string{"example.com/lib"},
		UnmappedTestImports: []string{"example.com/check"},
	}
	tests := []struct {
		name         string
		src          string
		old          []string
		wantLabels   []string
		wantUnmapped []string
	}{
		{
			name:         "a library wants what its sources need",
			src:          `fake_library(name = "lib")`,
			wantLabels:   []string{"//lib:lib"},
			wantUnmapped: []string{"example.com/lib"},
		},
		{
			name:         "a test on its own wants its library's deps and its own",
			src:          `fake_test(name = "test")`,
			wantLabels:   []string{"//lib:lib", "//check:check"},
			wantUnmapped: []string{"example.com/lib", "example.com/check"},
		},
		{
			name:         "a test with a target_under_test gets the library's deps through it",
			src:          `fake_test(name = "test", target_under_test = ":lib")`,
			wantLabels:   []string{"//check:check", "//lib:lib"},
			wantUnmapped: []string{"example.com/lib", "example.com/check"},
		},
		{
			name:         "a test with a same-package dep gets the library's deps through it",
			src:          `fake_test(name = "test")`,
			old:          []string{":lib"},
			wantLabels:   []string{"//check:check", "//lib:lib"},
			wantUnmapped: []string{"example.com/lib", "example.com/check"},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// No platforms: the fake's Linux dep stays out of it
			lang := &fakeLanguage{mapping: mapping}
			pkg, err := OpenPackage(lang, "/repo", "/repo/pkg", conditions.NewSpace(nil, ""))
			if err != nil {
				t.Fatal(err)
			}
			w := wantDeps(t, fakeTarget(t, pkg, tt.src), conditions.Configuration{}, tt.old)
			if !reflect.DeepEqual(w.Labels, tt.wantLabels) {
				t.Errorf("labels = %v, want %v", w.Labels, tt.wantLabels)
			}
			if !reflect.DeepEqual(w.Unmapped, tt.wantUnmapped) {
				t.Errorf("unmapped = %v, want %v", w.Unmapped, tt.wantUnmapped)
			}
		})
	}
}

// Deps on the package's own target are dropped: a Python member's imports
// of its own modules map to it.
func TestTargetDropsSelfReference(t *testing.T) {
	lang := &fakeLanguage{mapping: PackageMapping{
		Deps: []MappedDep{{Target: "//pkg:pkg"}, {Target: "//cfg:cfg"}},
	}}
	lib := fakeTarget(t, openFake(t, lang), `fake_library(name = "lib")`)
	if got, want := wantDeps(t, lib, onMacOS, nil).Labels, []string{"//cfg:cfg"}; !reflect.DeepEqual(got, want) {
		t.Errorf("labels = %v, want %v", got, want)
	}
}

// Deps sync doesn't own are wanted as unsynced, not as labels, and every
// resolution's reports are collected once each.
func TestTargetUnsyncedDepsAndReports(t *testing.T) {
	lang := &fakeLanguage{mapping: PackageMapping{
		UnmappedImports: []string{"example.com/x"},
		UnsyncedDeps:    []UnsyncedDep{{Dep: MappedDep{Target: "//build:build", ImportPath: "build"}, Reason: "build"}},
	}}
	pkg := openFake(t, lang)
	if err := pkg.ResolveAll(); err != nil {
		t.Fatal(err)
	}
	w := wantDeps(t, fakeTarget(t, pkg, `fake_library(name = "lib")`), onMacOS, nil)
	if want := []string{"//build:build"}; !reflect.DeepEqual(w.Unsynced, want) {
		t.Errorf("unsynced = %v, want %v", w.Unsynced, want)
	}
	want := []string{"unmapped import: example.com/x", "build dependency build not synced"}
	if got := pkg.Messages(); !reflect.DeepEqual(got, want) {
		t.Errorf("messages = %q, want %q", got, want)
	}
}

// The attributes a language owns are those any configuration's resolution
// sets, with their value in each.
func TestTargetOwnedAttributes(t *testing.T) {
	lang := &fakeLanguage{mapping: PackageMapping{Attrs: map[string][]string{"features": {"default"}}}}
	lib := fakeTarget(t, openFake(t, lang), `fake_library(name = "lib")`)
	owned, err := lib.Owned()
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"features"}; !reflect.DeepEqual(owned, want) {
		t.Fatalf("owned = %v, want %v", owned, want)
	}
	if got, err := lib.Attr(onLinux, "features"); err != nil || !reflect.DeepEqual(got, []string{"default"}) {
		t.Errorf("features = %v, %v; want [default]", got, err)
	}
}
