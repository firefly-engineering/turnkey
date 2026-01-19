package syncconfig

import (
	"os"
	"path/filepath"
	"testing"
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
enabled = false
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
	if !cfg.Deps[0].IsEnabled() {
		t.Error("expected first rule to be enabled")
	}

	// Check second rule is disabled
	if cfg.Deps[1].IsEnabled() {
		t.Error("expected second rule to be disabled")
	}
}

func TestEnabledRules(t *testing.T) {
	data := []byte(`
[[deps]]
name = "enabled"
sources = ["a"]
target = "b"
generator = ["cmd"]

[[deps]]
name = "disabled"
sources = ["a"]
target = "b"
generator = ["cmd"]
enabled = false

[[deps]]
name = "also-enabled"
sources = ["a"]
target = "b"
generator = ["cmd"]
enabled = true
`)

	cfg, err := Parse(data)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	enabled := cfg.EnabledDepsRules()
	if len(enabled) != 2 {
		t.Errorf("expected 2 enabled rules, got %d", len(enabled))
	}
	if enabled[0].Name != "enabled" {
		t.Errorf("expected first enabled rule to be 'enabled', got %q", enabled[0].Name)
	}
	if enabled[1].Name != "also-enabled" {
		t.Errorf("expected second enabled rule to be 'also-enabled', got %q", enabled[1].Name)
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
