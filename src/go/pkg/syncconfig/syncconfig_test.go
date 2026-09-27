package syncconfig

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

func TestParse(t *testing.T) {
	data := []byte(`
[[deps]]
name = "go"
sources = ["go.mod", "go.sum"]
target = "go-deps.toml"
generator = ["godeps-gen", "--go-mod", "go.mod", "--go-sum", "go.sum"]

[[deps]]
name = "rust"
sources = ["Cargo.toml", "Cargo.lock"]
target = "rust-deps.toml"
generator = ["cargo-deps-gen"]
`)

	cfg, err := Parse(data)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	if len(cfg.Deps) != 2 {
		t.Errorf("expected 2 deps rules, got %d", len(cfg.Deps))
	}

	// Check first rule
	if cfg.Deps[0].Name != "go" {
		t.Errorf("expected name 'go', got %q", cfg.Deps[0].Name)
	}
	if len(cfg.Deps[0].Sources) != 2 {
		t.Errorf("expected 2 sources, got %d", len(cfg.Deps[0].Sources))
	}
	if cfg.Deps[0].Target != "go-deps.toml" {
		t.Errorf("expected target 'go-deps.toml', got %q", cfg.Deps[0].Target)
	}
	if len(cfg.Deps[0].Generator) != 5 {
		t.Errorf("expected 5 generator args, got %d", len(cfg.Deps[0].Generator))
	}
}

func TestValidate(t *testing.T) {
	tests := []struct {
		name    string
		config  string
		wantErr bool
	}{
		{
			name: "valid",
			config: `
[[deps]]
name = "go"
sources = ["go.mod"]
target = "go-deps.toml"
generator = ["godeps-gen"]
`,
			wantErr: false,
		},
		{
			name: "missing name",
			config: `
[[deps]]
sources = ["go.mod"]
target = "go-deps.toml"
generator = ["godeps-gen"]
`,
			wantErr: true,
		},
		{
			name: "missing sources",
			config: `
[[deps]]
name = "go"
target = "go-deps.toml"
generator = ["godeps-gen"]
`,
			wantErr: true,
		},
		{
			name: "missing target",
			config: `
[[deps]]
name = "go"
sources = ["go.mod"]
generator = ["godeps-gen"]
`,
			wantErr: true,
		},
		{
			name: "missing generator",
			config: `
[[deps]]
name = "go"
sources = ["go.mod"]
target = "go-deps.toml"
`,
			wantErr: true,
		},
		{
			name: "language without a cell",
			config: `
[[languages]]
name = "go"
deps_file = "go-deps.toml"
`,
			wantErr: true,
		},
		{
			name: "language without a deps file",
			config: `
[[languages]]
name = "go"
cell = "godeps"
`,
			wantErr: true,
		},
		{
			name: "language listed twice",
			config: `
[[languages]]
name = "go"
cell = "godeps"
deps_file = "go-deps.toml"

[[languages]]
name = "go"
cell = "otherdeps"
deps_file = "go-deps.toml"
`,
			wantErr: true,
		},
		{
			name:    "empty config",
			config:  "",
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg, err := Parse([]byte(tt.config))
			if err != nil {
				t.Fatalf("Parse failed: %v", err)
			}

			err = cfg.Validate()
			if tt.wantErr && err == nil {
				t.Error("expected validation error, got nil")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("unexpected validation error: %v", err)
			}
		})
	}
}

func TestLoadDefault(t *testing.T) {
	// Test loading from a directory without config file
	cfg, err := LoadDefaultFrom("/nonexistent")
	if err != nil {
		t.Fatalf("LoadDefaultFrom failed: %v", err)
	}
	if len(cfg.Deps) != 0 {
		t.Errorf("expected empty deps, got %d", len(cfg.Deps))
	}

	// Test loading from a temp directory with config file
	dir := t.TempDir()
	tkDir := filepath.Join(dir, ".turnkey")
	if err := os.MkdirAll(tkDir, 0755); err != nil {
		t.Fatalf("failed to create .turnkey dir: %v", err)
	}

	configPath := filepath.Join(tkDir, "sync.toml")
	configData := []byte(`
[[deps]]
name = "test"
sources = ["test.txt"]
target = "out.txt"
generator = ["cat"]
`)
	if err := os.WriteFile(configPath, configData, 0644); err != nil {
		t.Fatalf("failed to write config: %v", err)
	}

	cfg, err = LoadDefaultFrom(dir)
	if err != nil {
		t.Fatalf("LoadDefaultFrom failed: %v", err)
	}
	if len(cfg.Deps) != 1 {
		t.Errorf("expected 1 deps rule, got %d", len(cfg.Deps))
	}
	if cfg.Deps[0].Name != "test" {
		t.Errorf("expected name 'test', got %q", cfg.Deps[0].Name)
	}
}

// A sync.toml generated before the [rules.go] settings were removed still
// loads: its section is ignored.
func TestLoadIgnoresRemovedRulesGo(t *testing.T) {
	path := filepath.Join(t.TempDir(), "sync.toml")
	content := `[rules]
enabled = true
auto_sync = false

[rules.go]
internal_prefix = "//src/go"
external_cell = "godeps"
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}
	cfg, err := Load(path)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if !cfg.Rules.Enabled || cfg.Rules.IsAutoSync() {
		t.Errorf("rules = %+v, want enabled without auto-sync", cfg.Rules)
	}
}

// The platforms (buck2.platforms) and the combined config_settings'
// package are read from [conditions].
func TestLoadConditions(t *testing.T) {
	cfg, err := Parse([]byte(`[conditions]
settings = "toolchains//conditions"

[[conditions.platforms]]
os = "linux"
cpu = "x86_64"

[[conditions.platforms]]
os = "macos"
cpu = "arm64"
`))
	if err != nil {
		t.Fatal(err)
	}
	want := []conditions.Platform{{OS: "linux", CPU: "x86_64"}, {OS: "macos", CPU: "arm64"}}
	if !reflect.DeepEqual(cfg.Conditions.Platforms, want) {
		t.Errorf("platforms = %+v, want %+v", cfg.Conditions.Platforms, want)
	}
	if got := len(cfg.Conditions.Space().Configurations); got != 2 {
		t.Errorf("space has %d configurations, want 2", got)
	}
}

// Each language's cell and deps file are read from [[languages]], in order.
func TestLoadLanguages(t *testing.T) {
	cfg, err := Parse([]byte(`[[languages]]
name = "go"
cell = "godeps"
deps_file = "go-deps.toml"

[[languages]]
name = "javascript"
cell = "jsdeps"
deps_file = "web/js-deps.toml"
`))
	if err != nil {
		t.Fatal(err)
	}
	want := []Language{
		{Name: "go", Cell: "godeps", DepsFile: "go-deps.toml"},
		{Name: "javascript", Cell: "jsdeps", DepsFile: "web/js-deps.toml"},
	}
	if !reflect.DeepEqual(cfg.Languages, want) {
		t.Errorf("languages = %+v, want %+v", cfg.Languages, want)
	}
	if err := cfg.Validate(); err != nil {
		t.Errorf("Validate: %v", err)
	}
}

func TestFindRoot(t *testing.T) {
	touch := func(path string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, nil, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	mkdir := func(path string) string {
		t.Helper()
		if err := os.MkdirAll(path, 0o755); err != nil {
			t.Fatal(err)
		}
		return path
	}

	t.Run("buckconfig", func(t *testing.T) {
		root := t.TempDir()
		touch(filepath.Join(root, ".buckconfig"))
		dir := mkdir(filepath.Join(root, "src", "cmd"))
		if got, ok := FindRoot(dir); !ok || got != root {
			t.Errorf("FindRoot = %q, %v; want %q, true", got, ok, root)
		}
	})

	t.Run("sync.toml", func(t *testing.T) {
		root := t.TempDir()
		touch(filepath.Join(root, DefaultConfigPath))
		dir := mkdir(filepath.Join(root, "src"))
		if got, ok := FindRoot(dir); !ok || got != root {
			t.Errorf("FindRoot = %q, %v; want %q, true", got, ok, root)
		}
	})

	t.Run("nearest wins", func(t *testing.T) {
		outer := t.TempDir()
		touch(filepath.Join(outer, ".buckconfig"))
		inner := filepath.Join(outer, "examples", "app")
		touch(filepath.Join(inner, DefaultConfigPath))
		dir := mkdir(filepath.Join(inner, "src"))
		if got, ok := FindRoot(dir); !ok || got != inner {
			t.Errorf("FindRoot = %q, %v; want %q, true", got, ok, inner)
		}
	})

	t.Run("a bare .turnkey is not a root", func(t *testing.T) {
		root := t.TempDir()
		dir := mkdir(filepath.Join(root, ".turnkey"))
		if got, ok := FindRoot(filepath.Dir(dir)); ok {
			t.Errorf("FindRoot = %q, true; want no root", got)
		}
	})
}
