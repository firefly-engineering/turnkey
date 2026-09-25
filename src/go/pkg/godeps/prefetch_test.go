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
	SupportedPaths []string
	Hashes         map[string]string
	Errors         map[string]error
	Calls          []string
}

func (m *MockPrefetcher) Supports(importPath string) bool {
	for _, p := range m.SupportedPaths {
		if p == importPath || p == "*" {
			return true
		}
	}
	return false
}

func (m *MockPrefetcher) Prefetch(importPath, version string) (string, error) {
	m.Calls = append(m.Calls, importPath+"@"+version)
	key := importPath + " " + version
	if err, ok := m.Errors[key]; ok {
		return "", err
	}
	if hash, ok := m.Hashes[key]; ok {
		return hash, nil
	}
	return "", errors.New("not found")
}

func TestPrefetchFunc(t *testing.T) {
	called := false
	f := PrefetchFunc(func(path, version string) (string, error) {
		called = true
		return "sha256-test=", nil
	})

	// Should always support
	if !f.Supports("anything") {
		t.Error("PrefetchFunc should support any path")
	}

	hash, err := f.Prefetch("test", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !called {
		t.Error("function was not called")
	}
	if hash != "sha256-test=" {
		t.Errorf("expected sha256-test=, got %s", hash)
	}
}

func TestPrefetchAll(t *testing.T) {
	mock := &MockPrefetcher{
		SupportedPaths: []string{"*"},
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
}

func TestPrefetchAll_WithErrors(t *testing.T) {
	mock := &MockPrefetcher{
		SupportedPaths: []string{"*"},
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

func TestGoProxyPrefetcher_Supports(t *testing.T) {
	p := &GoProxyPrefetcher{}

	// GoProxyPrefetcher is a fallback and supports everything
	tests := []string{
		"github.com/foo/bar",
		"golang.org/x/mod",
		"gopkg.in/yaml.v3",
		"example.com/anything",
		"bitbucket.org/user/repo",
		"gitlab.com/user/repo",
	}

	for _, path := range tests {
		t.Run(path, func(t *testing.T) {
			if !p.Supports(path) {
				t.Errorf("Supports(%s) = false, want true (fallback prefetcher)", path)
			}
		})
	}
}

func TestEscapeModulePath(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"github.com/foo/bar", "github.com/foo/bar"},
		{"github.com/Azure/azure-sdk", "github.com/!azure/azure-sdk"},
		{"github.com/BurntSushi/toml", "github.com/!burnt!sushi/toml"},
		{"ALLCAPS", "!a!l!l!c!a!p!s"},
		{"MixedCase/Path", "!mixed!case/!path"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result := escapeModulePath(tt.input)
			if result != tt.expected {
				t.Errorf("escapeModulePath(%s) = %s, want %s", tt.input, result, tt.expected)
			}
		})
	}
}

func TestDefaultPrefetcherHashesTheProxyZipTheCellFetches(t *testing.T) {
	// A stand-in nix-prefetch-cached that records its arguments
	bin := t.TempDir()
	calls := filepath.Join(bin, "calls")
	script := "#!/bin/sh\necho \"$@\" >> " + calls + "\necho sha256-stand-in=\n"
	if err := os.WriteFile(filepath.Join(bin, "nix-prefetch-cached"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))

	for _, noCache := range []bool{false, true} {
		hash, err := DefaultPrefetcher(nil, noCache).Prefetch("github.com/BurntSushi/toml", "v1.4.0")
		if err != nil || hash != "sha256-stand-in=" {
			t.Fatalf("Prefetch = %q, %v", hash, err)
		}
	}

	got, _ := os.ReadFile(calls)
	// The URL nix/lib/deps-cell/fetchers.nix's goproxy fetcher builds,
	// unpacked as fetchzip unpacks it
	url := "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip"
	want := "--unpack " + url + "\n--unpack --no-cache " + url + "\n"
	if string(got) != want {
		t.Errorf("nix-prefetch-cached called with\n%s\nwant\n%s", got, want)
	}
}

func TestPrefetchFailureNamesTheURL(t *testing.T) {
	bin := t.TempDir()
	script := "#!/bin/sh\necho 'no such module' >&2\nexit 1\n"
	if err := os.WriteFile(filepath.Join(bin, "nix-prefetch-cached"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin)

	_, err := DefaultPrefetcher(nil, false).Prefetch("example.com/gone", "v1.0.0")
	if err == nil || !strings.Contains(err.Error(), "proxy.golang.org/example.com/gone/@v/v1.0.0.zip") || !strings.Contains(err.Error(), "no such module") {
		t.Errorf("error %v doesn't name the URL and nix-prefetch-cached's reason", err)
	}
}
