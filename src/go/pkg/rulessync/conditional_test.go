package rulessync

import (
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/mapper"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// turnkey's default platforms
var defaultPlatforms = []conditions.Platform{
	{OS: "linux", CPU: "x86_64"},
	{OS: "linux", CPU: "arm64"},
	{OS: "macos", CPU: "x86_64"},
	{OS: "macos", CPU: "arm64"},
}

// fakeLanguage is a plug-in whose deps depend on the OS: every platform
// needs //unix:unix and Linux also //linux:only. A target's "features"
// variant adds //feature/<name>:<name> per feature.
type fakeLanguage struct {
	// requests records what was resolved.
	requests []mapper.Request
}

func (l *fakeLanguage) Name() string { return "fake" }

func (l *fakeLanguage) RuleKind(rule string) (mapper.TargetKind, bool) {
	return mapper.Library, rule == "fake_library"
}

func (l *fakeLanguage) DepsAttribute() string    { return "deps" }
func (l *fakeLanguage) SourcePatterns() []string { return []string{"*.fake"} }

func (l *fakeLanguage) Dimensions(string) ([]string, error) {
	return []string{conditions.OS}, nil
}

func (l *fakeLanguage) VariantAttributes(mapper.TargetKind) []string { return []string{"features"} }

func (l *fakeLanguage) ResolveDeps(_ string, req mapper.Request) (mapper.PackageMapping, error) {
	l.requests = append(l.requests, req)
	deps := []mapper.MappedDep{{Target: "//unix:unix"}}
	if req.Config[conditions.OS] == "linux" {
		deps = append(deps, mapper.MappedDep{Target: "//linux:only"})
	}
	if features, ok := starlark.Labels(req.Variant["features"]); ok {
		for _, f := range features {
			deps = append(deps, mapper.MappedDep{Target: "//feature/" + f + ":" + f})
		}
	}
	return mapper.PackageMapping{Deps: deps}, nil
}

// syncFake syncs rules.star content with the fake language and returns
// the result and what sync wrote.
func syncFake(t *testing.T, lang *fakeLanguage, platforms []conditions.Platform, content string) (*SyncResult, string) {
	t.Helper()
	root := t.TempDir()
	rulesPath := filepath.Join(root, "pkg/rules.star")
	writeFiles(t, root, map[string]string{"pkg/rules.star": content})

	s, err := newSyncer(Config{
		ProjectRoot: root,
		Force:       true,
		Conditions:  &syncconfig.ConditionsConfig{Platforms: platforms},
	}, mapper.NewWith(mapper.Config{ProjectRoot: root}, lang))
	if err != nil {
		t.Fatal(err)
	}
	result, err := s.SyncFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	out, err := os.ReadFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	return result, string(out)
}

const fakeLib = `fake_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//old:dep",
        # turnkey:auto-end
    ],
)
`

// Deps that differ by OS are written as [<common>] + select({...}) keyed
// on the OS, with a branch for every OS and no DEFAULT; syncing again
// changes nothing, and the package is resolved once per OS.
func TestSyncFileWritesPlatformConditionalDeps(t *testing.T) {
	lang := &fakeLanguage{}
	result, out := syncFake(t, lang, defaultPlatforms, fakeLib)
	want := `fake_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//unix:unix",
        # turnkey:auto-end
    ] + select({
        "config//os:linux": ["//linux:only"],
        "config//os:macos": [],
    }),
)
`
	if out != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", out, want)
	}
	if !result.Updated {
		t.Error("result not updated")
	}
	if len(lang.requests) != 2 {
		t.Errorf("resolved %d times, want once per OS: %+v", len(lang.requests), lang.requests)
	}

	result, again := syncFake(t, &fakeLanguage{}, defaultPlatforms, out)
	if result.Updated || again != out || len(result.Changes) != 0 {
		t.Errorf("second sync changed the file (%+v):\n%s", result.Changes, again)
	}
}

// What sync writes doesn't depend on the host: pretending to be each
// supported platform gives the same file.
func TestSyncFileIsHostIndependent(t *testing.T) {
	var outputs []string
	for _, host := range []struct{ goos, goarch string }{
		{"linux", "amd64"}, {"linux", "arm64"}, {"darwin", "amd64"}, {"darwin", "arm64"},
	} {
		t.Setenv("GOOS", host.goos)
		t.Setenv("GOARCH", host.goarch)
		_, out := syncFake(t, &fakeLanguage{}, defaultPlatforms, fakeLib)
		outputs = append(outputs, out)
	}
	for i, out := range outputs[1:] {
		if out != outputs[0] {
			t.Errorf("host %d wrote:\n%s\nhost 0 wrote:\n%s", i+1, out, outputs[0])
		}
	}
}

// Without platforms there's a single configuration: a language's
// conditions can't show, and the deps are a plain list.
func TestSyncFileWithoutPlatforms(t *testing.T) {
	_, out := syncFake(t, &fakeLanguage{}, nil, fakeLib)
	if want := "\"//unix:unix\",\n        # turnkey:auto-end\n    ],\n)"; !strings.Contains(out, want) {
		t.Errorf("rules.star:\n%s\nwant a plain list", out)
	}
}

// A variant attribute is read in each configuration, a select() included,
// and passed to the language.
func TestSyncFilePassesVariants(t *testing.T) {
	lang := &fakeLanguage{}
	_, out := syncFake(t, lang, defaultPlatforms, `fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
    deps = [],
)
`)
	want := `fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
    deps = [
        "//unix:unix",
        "//feature/a:a",
    ] + select({
        "config//os:linux": ["//linux:only"],
        "config//os:macos": ["//feature/b:b"],
    }),
)
`
	if out != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", out, want)
	}
	var variants []string
	for _, req := range lang.requests {
		if v, ok := req.Variant["features"]; ok {
			variants = append(variants, req.Config[conditions.OS]+":"+starlark.Render(v))
		}
	}
	slices.Sort(variants)
	if w := []string{`linux:["a"]`, "macos:[\n    \"a\",\n    \"b\",\n]"}; !reflect.DeepEqual(variants, w) {
		t.Errorf("variants = %q, want %q", variants, w)
	}
}

// A variant attribute whose select() has a key sync doesn't know makes the
// target unreadable.
func TestSyncFileUnreadableVariant(t *testing.T) {
	result, _ := syncFake(t, &fakeLanguage{}, defaultPlatforms, `fake_library(
    name = "lib",
    features = select({"//my:setting": ["a"]}),
    deps = [],
)
`)
	want := []UnreadableTarget{{Target: "lib", Attribute: "features"}}
	if !reflect.DeepEqual(result.Unreadable, want) {
		t.Errorf("unreadable = %+v, want %+v", result.Unreadable, want)
	}
}

// A crate's cfg(target_os = "linux") and cfg(unix) deps: the unix one is
// common, the Linux one is keyed on the OS, and there is no DEFAULT.
func TestSyncFileRustTargetSpecificDeps(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"Cargo.toml": "[workspace]\nmembers = [\"crates/*\"]\n",
		"rust-deps.toml": `[deps."libc@0.2.0"]
name = "libc"
[deps."inotify@0.11.0"]
name = "inotify"
`,
		"crates/watch/Cargo.toml": `[package]
name = "watch"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "linux")'.dependencies]
inotify = "0.11"
`,
		"crates/watch/rules.star": `rust_library(
    name = "watch",
    deps = [],
)
`,
	})
	s, err := NewSyncer(Config{
		ProjectRoot: root,
		Force:       true,
		Conditions:  &syncconfig.ConditionsConfig{Platforms: defaultPlatforms},
	})
	if err != nil {
		t.Fatal(err)
	}
	rulesPath := filepath.Join(root, "crates/watch/rules.star")
	result, err := s.SyncFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	out, err := os.ReadFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	want := `rust_library(
    name = "watch",
    deps = ["rustdeps//vendor/libc:libc"] + select({
        "config//os:linux": ["rustdeps//vendor/inotify:inotify"],
        "config//os:macos": [],
    }),
)
`
	if string(out) != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", out, want)
	}
}

// A Rust variant target with a select()'d cargo_features gets the features
// and deps its request expands to, per OS; the primary target gets the
// defaults' features.
func TestSyncFileRustVariantTarget(t *testing.T) {
	root := t.TempDir()
	writeFiles(t, root, map[string]string{
		"Cargo.toml": "[workspace]\nmembers = [\"crates/*\"]\n",
		"rust-deps.toml": `[deps."libc@0.2.0"]
name = "libc"
[deps."fuser@0.15.0"]
name = "fuser"
[deps."notify@8.0.0"]
name = "notify"
[deps."log@0.4.0"]
name = "log"
`,
		"crates/comp/Cargo.toml": `[package]
name = "comp"

[features]
default = ["std"]
std = []
fuse = ["dep:fuser", "dep:libc"]
fuse-t = ["dep:libc"]
watcher = ["dep:notify"]

[dependencies]
log = "0.4"
fuser = { version = "0.15", optional = true }
libc = { version = "0.2", optional = true }
notify = { version = "8", optional = true }
`,
		"crates/comp/rules.star": `rust_library(
    name = "comp",
    deps = ["rustdeps//vendor/log:log"],
)

rust_library(
    name = "comp-full",
    cargo_features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
    deps = [],
)
`,
	})
	s, err := NewSyncer(Config{
		ProjectRoot: root,
		Force:       true,
		Conditions:  &syncconfig.ConditionsConfig{Platforms: defaultPlatforms},
	})
	if err != nil {
		t.Fatal(err)
	}
	rulesPath := filepath.Join(root, "crates/comp/rules.star")
	result, err := s.SyncFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Errors) != 0 {
		t.Fatalf("sync errors: %v", result.Errors)
	}
	out, err := os.ReadFile(rulesPath)
	if err != nil {
		t.Fatal(err)
	}
	want := `rust_library(
    name = "comp",
    deps = ["rustdeps//vendor/log:log"],
    features = ["std"],
)

rust_library(
    name = "comp-full",
    cargo_features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
    deps = [
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/notify:notify",
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        "config//os:macos": [],
    }),
    features = [
        "std",
        "watcher",
    ] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
)
`
	if string(out) != want {
		t.Errorf("rules.star:\n%s\nwant:\n%s", out, want)
	}
}
