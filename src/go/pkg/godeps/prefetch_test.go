package godeps

import (
	"bytes"
	"errors"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/prefetchcache"
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

func TestGitHubPrefetcher_Supports(t *testing.T) {
	p := &GitHubPrefetcher{}

	tests := []struct {
		path     string
		expected bool
	}{
		{"github.com/foo/bar", true},
		{"github.com/owner/repo/subpkg", true},
		{"golang.org/x/mod", false},
		{"gopkg.in/yaml.v3", false},
		{"example.com/pkg", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if p.Supports(tt.path) != tt.expected {
				t.Errorf("Supports(%s) = %v, want %v", tt.path, !tt.expected, tt.expected)
			}
		})
	}
}

func TestGolangOrgPrefetcher_Supports(t *testing.T) {
	p := &GolangOrgPrefetcher{}

	tests := []struct {
		path     string
		expected bool
	}{
		{"golang.org/x/mod", true},
		{"golang.org/x/tools", true},
		{"golang.org/x/crypto/bcrypt", true},
		{"github.com/foo/bar", false},
		{"golang.org/something", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if p.Supports(tt.path) != tt.expected {
				t.Errorf("Supports(%s) = %v, want %v", tt.path, !tt.expected, tt.expected)
			}
		})
	}
}

func TestChainPrefetcher_Supports(t *testing.T) {
	chain := ChainPrefetcher{
		&MockPrefetcher{SupportedPaths: []string{"github.com/foo/bar"}},
		&MockPrefetcher{SupportedPaths: []string{"golang.org/x/mod"}},
	}

	tests := []struct {
		path     string
		expected bool
	}{
		{"github.com/foo/bar", true},
		{"golang.org/x/mod", true},
		{"unsupported.com/pkg", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if chain.Supports(tt.path) != tt.expected {
				t.Errorf("Supports(%s) = %v, want %v", tt.path, !tt.expected, tt.expected)
			}
		})
	}
}

func TestChainPrefetcher_Prefetch(t *testing.T) {
	first := &MockPrefetcher{
		SupportedPaths: []string{"github.com/first/pkg"},
		Hashes:         map[string]string{"github.com/first/pkg v1.0.0": "sha256-first="},
	}
	second := &MockPrefetcher{
		SupportedPaths: []string{"github.com/second/pkg"},
		Hashes:         map[string]string{"github.com/second/pkg v1.0.0": "sha256-second="},
	}
	chain := ChainPrefetcher{first, second}

	t.Run("first prefetcher", func(t *testing.T) {
		hash, err := chain.Prefetch("github.com/first/pkg", "v1.0.0")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if hash != "sha256-first=" {
			t.Errorf("expected sha256-first=, got %s", hash)
		}
	})

	t.Run("second prefetcher", func(t *testing.T) {
		hash, err := chain.Prefetch("github.com/second/pkg", "v1.0.0")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if hash != "sha256-second=" {
			t.Errorf("expected sha256-second=, got %s", hash)
		}
	})

	t.Run("unsupported path", func(t *testing.T) {
		_, err := chain.Prefetch("unsupported.com/pkg", "v1.0.0")
		if err == nil {
			t.Error("expected error for unsupported path")
		}
	})
}

func TestChainPrefetcher_TriesFallback(t *testing.T) {
	// First prefetcher supports the path but fails
	failing := &MockPrefetcher{
		SupportedPaths: []string{"github.com/test/pkg"},
		Errors:         map[string]error{"github.com/test/pkg v1.0.0": errors.New("first failed")},
	}
	// Second prefetcher also supports it and succeeds
	succeeding := &MockPrefetcher{
		SupportedPaths: []string{"github.com/test/pkg"},
		Hashes:         map[string]string{"github.com/test/pkg v1.0.0": "sha256-success="},
	}
	chain := ChainPrefetcher{failing, succeeding}

	hash, err := chain.Prefetch("github.com/test/pkg", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if hash != "sha256-success=" {
		t.Errorf("expected sha256-success=, got %s", hash)
	}
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

func TestParseGitHubPath(t *testing.T) {
	tests := []struct {
		path          string
		expectedOwner string
		expectedRepo  string
		expectError   bool
	}{
		{"github.com/foo/bar", "foo", "bar", false},
		{"github.com/owner/repo/subpkg", "owner", "repo", false},
		{"github.com/org/repo-name", "org", "repo-name", false},
		{"golang.org/x/mod", "", "", true},
		{"github.com/onlyowner", "", "", true},
		{"not-github", "", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			owner, repo, err := parseGitHubPath(tt.path)
			if tt.expectError {
				if err == nil {
					t.Error("expected error")
				}
			} else {
				if err != nil {
					t.Fatalf("unexpected error: %v", err)
				}
				if owner != tt.expectedOwner {
					t.Errorf("owner: expected %s, got %s", tt.expectedOwner, owner)
				}
				if repo != tt.expectedRepo {
					t.Errorf("repo: expected %s, got %s", tt.expectedRepo, repo)
				}
			}
		})
	}
}

func TestGopkgInPrefetcher_Supports(t *testing.T) {
	p := &GopkgInPrefetcher{}

	tests := []struct {
		path     string
		expected bool
	}{
		{"gopkg.in/yaml.v3", true},
		{"gopkg.in/check.v1", true},
		{"gopkg.in/user/pkg.v2", true},
		{"github.com/foo/bar", false},
		{"golang.org/x/mod", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if p.Supports(tt.path) != tt.expected {
				t.Errorf("Supports(%s) = %v, want %v", tt.path, !tt.expected, tt.expected)
			}
		})
	}
}

func TestParseGopkgInPath(t *testing.T) {
	tests := []struct {
		path          string
		expectedOwner string
		expectedRepo  string
		expectError   bool
	}{
		// Single-element paths: gopkg.in/name.vN -> go-name/name
		{"gopkg.in/yaml.v3", "go-yaml", "yaml", false},
		{"gopkg.in/check.v1", "go-check", "check", false},
		{"gopkg.in/ini.v1", "go-ini", "ini", false},
		// Two-element paths: gopkg.in/user/pkg.vN -> user/pkg
		{"gopkg.in/user/pkg.v2", "user", "pkg", false},
		{"gopkg.in/natefinch/lumberjack.v2", "natefinch", "lumberjack", false},
		// Subpackages
		{"gopkg.in/user/pkg.v2/subpkg", "user", "pkg", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			owner, repo, err := parseGopkgInPath(tt.path)
			if tt.expectError {
				if err == nil {
					t.Error("expected error")
				}
			} else {
				if err != nil {
					t.Fatalf("unexpected error: %v", err)
				}
				if owner != tt.expectedOwner {
					t.Errorf("owner: expected %s, got %s", tt.expectedOwner, owner)
				}
				if repo != tt.expectedRepo {
					t.Errorf("repo: expected %s, got %s", tt.expectedRepo, repo)
				}
			}
		})
	}
}

func TestStripVersionSuffix(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"yaml.v3", "yaml"},
		{"check.v1", "check"},
		{"pkg.v10", "pkg"},
		{"lumberjack.v2", "lumberjack"},
		{"noversion", "noversion"},
		{"has.dot", "has.dot"},
		{"pkg.vx", "pkg.vx"}, // 'x' is not a digit
		{".v1", ""},          // Edge case: just version suffix
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result := stripVersionSuffix(tt.input)
			if result != tt.expected {
				t.Errorf("stripVersionSuffix(%s) = %s, want %s", tt.input, result, tt.expected)
			}
		})
	}
}

func TestUberGoPrefetcher_Supports(t *testing.T) {
	p := &UberGoPrefetcher{}

	tests := []struct {
		path     string
		expected bool
	}{
		{"go.uber.org/zap", true},
		{"go.uber.org/atomic", true},
		{"go.uber.org/multierr", true},
		{"github.com/uber-go/zap", false},
		{"golang.org/x/mod", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if p.Supports(tt.path) != tt.expected {
				t.Errorf("Supports(%s) = %v, want %v", tt.path, !tt.expected, tt.expected)
			}
		})
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

func TestDefaultPrefetcher(t *testing.T) {
	var buf bytes.Buffer
	p := DefaultPrefetcher(&buf)

	// Should support golang.org/x paths
	if !p.Supports("golang.org/x/mod") {
		t.Error("should support golang.org/x/*")
	}

	// Should support github.com paths
	if !p.Supports("github.com/foo/bar") {
		t.Error("should support github.com/*")
	}

	// Should support gopkg.in paths
	if !p.Supports("gopkg.in/yaml.v3") {
		t.Error("should support gopkg.in/*")
	}

	// Should support go.uber.org paths
	if !p.Supports("go.uber.org/zap") {
		t.Error("should support go.uber.org/*")
	}

	// Should now support any path via GoProxy fallback
	if !p.Supports("example.com/pkg") {
		t.Error("should support example.com/* via GoProxy fallback")
	}
}

func TestCachedPrefetcher(t *testing.T) {
	// Create a mock inner prefetcher
	inner := &MockPrefetcher{
		SupportedPaths: []string{"*"},
		Hashes: map[string]string{
			"github.com/foo/bar v1.0.0": "sha256-foo=",
			"github.com/baz/qux v1.0.0": "sha256-baz=",
		},
	}

	// Create cache in temp directory
	dir := t.TempDir()
	cache, err := prefetchcache.WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	var logBuf bytes.Buffer
	cached := &CachedPrefetcher{
		Inner:  inner,
		Cache:  cache,
		Logger: &logBuf,
	}

	// First prefetch - should be a cache miss
	hash, err := cached.Prefetch("github.com/foo/bar", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if hash != "sha256-foo=" {
		t.Errorf("expected sha256-foo=, got %s", hash)
	}
	if len(inner.Calls) != 1 {
		t.Errorf("expected 1 inner call, got %d", len(inner.Calls))
	}

	// Second prefetch of same dep - should be a cache hit
	logBuf.Reset()
	hash, err = cached.Prefetch("github.com/foo/bar", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if hash != "sha256-foo=" {
		t.Errorf("expected sha256-foo=, got %s", hash)
	}
	// Inner should NOT have been called again
	if len(inner.Calls) != 1 {
		t.Errorf("expected still 1 inner call (cache hit), got %d", len(inner.Calls))
	}
	if !bytes.Contains(logBuf.Bytes(), []byte("cache hit")) {
		t.Error("expected cache hit message in log")
	}

	// Different dep - should be a cache miss
	hash, err = cached.Prefetch("github.com/baz/qux", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if hash != "sha256-baz=" {
		t.Errorf("expected sha256-baz=, got %s", hash)
	}
	if len(inner.Calls) != 2 {
		t.Errorf("expected 2 inner calls, got %d", len(inner.Calls))
	}
}

func TestCachedPrefetcher_Persistence(t *testing.T) {
	inner := &MockPrefetcher{
		SupportedPaths: []string{"*"},
		Hashes: map[string]string{
			"github.com/test/pkg v1.0.0": "sha256-test=",
		},
	}

	dir := t.TempDir()

	// First session: prefetch and save
	cache1, _ := prefetchcache.WithDir(dir)
	cached1 := &CachedPrefetcher{Inner: inner, Cache: cache1}

	_, err := cached1.Prefetch("github.com/test/pkg", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(inner.Calls) != 1 {
		t.Fatalf("expected 1 inner call, got %d", len(inner.Calls))
	}

	// Save cache
	cache1.Save()

	// Second session: load cache, should be a hit
	cache2, _ := prefetchcache.WithDir(dir)
	cached2 := &CachedPrefetcher{Inner: inner, Cache: cache2}

	hash, err := cached2.Prefetch("github.com/test/pkg", "v1.0.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if hash != "sha256-test=" {
		t.Errorf("expected sha256-test=, got %s", hash)
	}
	// Inner should NOT have been called again (cache was loaded from disk)
	if len(inner.Calls) != 1 {
		t.Errorf("expected still 1 inner call (loaded from disk), got %d", len(inner.Calls))
	}
}
