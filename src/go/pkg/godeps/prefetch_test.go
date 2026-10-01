package godeps

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// MockPrefetcher is a test double for Prefetcher
type MockPrefetcher struct {
	Hashes map[string]string
	Errors map[string]error
	Calls  [][]Module
}

func (m *MockPrefetcher) Prefetch(mods []Module) []Prefetched {
	m.Calls = append(m.Calls, mods)
	results := make([]Prefetched, len(mods))
	for i, mod := range mods {
		key := mod.Path + " " + mod.Version
		if err, ok := m.Errors[key]; ok {
			results[i].Err = err
		} else if hash, ok := m.Hashes[key]; ok {
			results[i].Hash = hash
		} else {
			results[i].Err = errors.New("not found")
		}
	}
	return results
}

func TestPrefetchAllMakesOneCall(t *testing.T) {
	mock := &MockPrefetcher{
		Hashes: map[string]string{
			"github.com/foo/bar v1.0.0": "sha256-foo=",
			"github.com/baz/qux v1.0.0": "sha256-baz=",
		},
	}

	deps := []Dependency{
		{ImportPath: "github.com/foo/bar", Version: "v1.0.0"},
		{ImportPath: "github.com/baz/qux", Version: "v1.0.0"},
	}

	PrefetchAll(deps, mock, nil)

	if deps[0].NixHash != "sha256-foo=" {
		t.Errorf("expected sha256-foo=, got %s", deps[0].NixHash)
	}
	if deps[1].NixHash != "sha256-baz=" {
		t.Errorf("expected sha256-baz=, got %s", deps[1].NixHash)
	}
	if len(mock.Calls) != 1 {
		t.Errorf("expected one batch call, got %d", len(mock.Calls))
	}
}

func TestPrefetchAll_WithErrors(t *testing.T) {
	mock := &MockPrefetcher{
		Hashes: map[string]string{
			"github.com/good/pkg v1.0.0": "sha256-good=",
		},
		Errors: map[string]error{
			"github.com/bad/pkg v1.0.0": errors.New("fetch failed"),
		},
	}

	deps := []Dependency{
		{ImportPath: "github.com/good/pkg", Version: "v1.0.0"},
		{ImportPath: "github.com/bad/pkg", Version: "v1.0.0"},
	}

	var errorsReceived []string
	errHandler := func(dep Dependency, err error) {
		errorsReceived = append(errorsReceived, dep.ImportPath)
	}

	PrefetchAll(deps, mock, errHandler)

	if deps[0].NixHash != "sha256-good=" {
		t.Errorf("expected sha256-good=, got %s", deps[0].NixHash)
	}
	if deps[1].NixHash != "" {
		t.Errorf("expected empty hash for failed dep, got %s", deps[1].NixHash)
	}
	if len(errorsReceived) != 1 || errorsReceived[0] != "github.com/bad/pkg" {
		t.Errorf("expected error for bad/pkg, got %v", errorsReceived)
	}
}

func TestProxyZipURLEscapesPathAndVersion(t *testing.T) {
	t.Parallel()
	tests := []struct {
		mod  Module
		want string
	}{
		{Module{"github.com/foo/bar", "v1.0.0"}, "https://proxy.golang.org/github.com/foo/bar/@v/v1.0.0.zip"},
		{Module{"github.com/BurntSushi/toml", "v1.4.0"}, "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip"},
		{Module{"github.com/Azure/azure-sdk", "v1.0.0-RC1"}, "https://proxy.golang.org/github.com/!azure/azure-sdk/@v/v1.0.0-!r!c1.zip"},
	}
	for _, tt := range tests {
		got, err := proxyZipURL(tt.mod)
		if err != nil || got != tt.want {
			t.Errorf("proxyZipURL(%v) = %q, %v; want %q", tt.mod, got, err, tt.want)
		}
	}
}

func TestGoProxyPrefetcherFailsAModuleItCannotEscape(t *testing.T) {
	t.Parallel()
	command := standIn(t, `while read -r url; do echo "sha256-${url##*/}"; done
`)
	p := &GoProxyPrefetcher{Command: command}
	got := p.Prefetch([]Module{
		{Path: "github.com/foo/bar", Version: "v1.0.0!"},
		{Path: "golang.org/x/mod", Version: "v0.20.0"},
	})
	if len(got) != 2 || got[0].Err == nil || got[1] != (Prefetched{Hash: "sha256-v0.20.0.zip"}) {
		t.Errorf("Prefetch = %v, want an error then a hash", got)
	}
}

// standIn writes a stand-in nix-prefetch-cached running script, and
// returns its path
func standIn(t *testing.T, script string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "nix-prefetch-cached")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+script), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestGoProxyPrefetcherHashesTheProxyZipsTheCellFetches(t *testing.T) {
	t.Parallel()
	// Records its arguments and stdin, and answers one hash per URL
	dir := t.TempDir()
	calls := filepath.Join(dir, "calls")
	command := standIn(t, `echo "$@" >> `+calls+`
while read -r url; do echo "$url" >> `+calls+`; echo "sha256-${url##*/}"; done
`)

	mods := []Module{
		{Path: "github.com/BurntSushi/toml", Version: "v1.4.0"},
		{Path: "golang.org/x/mod", Version: "v0.20.0"},
	}
	for _, noCache := range []bool{false, true} {
		p := &GoProxyPrefetcher{Command: command, NoCache: noCache}
		got := p.Prefetch(mods)
		want := []Prefetched{{Hash: "sha256-v1.4.0.zip"}, {Hash: "sha256-v0.20.0.zip"}}
		if len(got) != 2 || got[0] != want[0] || got[1] != want[1] {
			t.Fatalf("Prefetch = %v, want %v", got, want)
		}
	}

	got, _ := os.ReadFile(calls)
	// The URLs nix/lib/deps-cell/fetchers.nix's goproxy fetcher builds,
	// unpacked as fetchzip unpacks them, all in one call
	urls := "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip\n" +
		"https://proxy.golang.org/golang.org/x/mod/@v/v0.20.0.zip\n"
	want := "--batch --unpack\n" + urls + "--batch --unpack --no-cache\n" + urls
	if string(got) != want {
		t.Errorf("nix-prefetch-cached called with\n%s\nwant\n%s", got, want)
	}
}

func TestGoProxyPrefetcherFailsOnlyTheModuleThatFailed(t *testing.T) {
	t.Parallel()
	command := standIn(t, `read -r a; read -r b
echo sha256-good=
echo "error: no such module"
`)

	got := (&GoProxyPrefetcher{Command: command}).Prefetch([]Module{
		{Path: "example.com/good", Version: "v1.0.0"},
		{Path: "example.com/gone", Version: "v1.0.0"},
	})
	if got[0].Hash != "sha256-good=" || got[0].Err != nil {
		t.Errorf("good module = %v", got[0])
	}
	err := got[1].Err
	if err == nil || !strings.Contains(err.Error(), "proxy.golang.org/example.com/gone/@v/v1.0.0.zip") || !strings.Contains(err.Error(), "no such module") {
		t.Errorf("error %v doesn't name the URL and nix-prefetch-cached's reason", err)
	}
}

func TestGoProxyPrefetcherFailsEveryModuleWhenTheBatchFails(t *testing.T) {
	t.Parallel()
	command := standIn(t, "echo 'cache dir unwritable' >&2\nexit 1\n")

	got := (&GoProxyPrefetcher{Command: command}).Prefetch([]Module{
		{Path: "example.com/a", Version: "v1.0.0"},
		{Path: "example.com/b", Version: "v1.0.0"},
	})
	for i, r := range got {
		if r.Err == nil || !strings.Contains(r.Err.Error(), "cache dir unwritable") {
			t.Errorf("module %d: error %v doesn't carry nix-prefetch-cached's reason", i, r.Err)
		}
	}
}
