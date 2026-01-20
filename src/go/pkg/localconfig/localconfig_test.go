package localconfig

import (
	"testing"
)

func TestParse(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantErr  bool
		validate func(*Config) error
	}{
		{
			name:    "empty config",
			input:   "",
			wantErr: false,
			validate: func(c *Config) error {
				if len(c.Run) != 0 {
					t.Errorf("expected empty Run map, got %d entries", len(c.Run))
				}
				return nil
			},
		},
		{
			name: "run override",
			input: `
[run."//docs/user-manual"]
args = ["-n", "100.64.25.26"]
`,
			wantErr: false,
			validate: func(c *Config) error {
				override := c.GetOverride("run", "//docs/user-manual")
				if override == nil {
					t.Error("expected override for //docs/user-manual")
					return nil
				}
				if len(override.Args) != 2 {
					t.Errorf("expected 2 args, got %d", len(override.Args))
				}
				if override.Args[0] != "-n" || override.Args[1] != "100.64.25.26" {
					t.Errorf("unexpected args: %v", override.Args)
				}
				return nil
			},
		},
		{
			name: "multiple commands",
			input: `
[run."//target:a"]
args = ["--run-flag"]

[build."//target:b"]
args = ["--build-flag"]

[test."//target:c"]
args = ["--test-flag"]
`,
			wantErr: false,
			validate: func(c *Config) error {
				if len(c.Run) != 1 {
					t.Errorf("expected 1 run override, got %d", len(c.Run))
				}
				if len(c.Build) != 1 {
					t.Errorf("expected 1 build override, got %d", len(c.Build))
				}
				if len(c.Test) != 1 {
					t.Errorf("expected 1 test override, got %d", len(c.Test))
				}
				return nil
			},
		},
		{
			name: "pattern match with ...",
			input: `
[test."//src/pkg/..."]
args = ["--verbose"]
`,
			wantErr: false,
			validate: func(c *Config) error {
				override := c.GetOverride("test", "//src/pkg/foo:bar")
				if override == nil {
					t.Error("expected pattern match for //src/pkg/foo:bar")
					return nil
				}
				if override.Args[0] != "--verbose" {
					t.Errorf("unexpected args: %v", override.Args)
				}
				return nil
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg, err := Parse([]byte(tt.input))
			if (err != nil) != tt.wantErr {
				t.Errorf("Parse() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if cfg != nil && tt.validate != nil {
				tt.validate(cfg)
			}
		})
	}
}

func TestMatchTarget(t *testing.T) {
	tests := []struct {
		pattern string
		target  string
		want    bool
	}{
		{"//foo:bar", "//foo:bar", true},
		{"//foo:bar", "//foo:baz", false},
		{"//foo/...", "//foo/bar:baz", true},
		{"//foo/...", "//foo:bar", true},
		{"//foo/...", "//bar:baz", false},
		{"//...", "//anything:here", true},
	}

	for _, tt := range tests {
		t.Run(tt.pattern+"_"+tt.target, func(t *testing.T) {
			if got := matchTarget(tt.pattern, tt.target); got != tt.want {
				t.Errorf("matchTarget(%q, %q) = %v, want %v", tt.pattern, tt.target, got, tt.want)
			}
		})
	}
}

func TestGetOverride(t *testing.T) {
	cfg := &Config{
		Run: map[string]TargetOverride{
			"//docs/user-manual": {Args: []string{"-n", "localhost"}},
		},
		Build: map[string]TargetOverride{
			"//src/...": {Args: []string{"--debug"}},
		},
		Test: make(map[string]TargetOverride),
	}

	// Exact match
	if o := cfg.GetOverride("run", "//docs/user-manual"); o == nil {
		t.Error("expected override for run //docs/user-manual")
	}

	// Pattern match
	if o := cfg.GetOverride("build", "//src/pkg:foo"); o == nil {
		t.Error("expected override for build //src/pkg:foo via pattern")
	}

	// No match
	if o := cfg.GetOverride("run", "//other:target"); o != nil {
		t.Error("expected no override for run //other:target")
	}

	// Unknown command
	if o := cfg.GetOverride("install", "//docs/user-manual"); o != nil {
		t.Error("expected no override for unknown command")
	}
}

func TestHasOverrides(t *testing.T) {
	empty := &Config{
		Run:   make(map[string]TargetOverride),
		Build: make(map[string]TargetOverride),
		Test:  make(map[string]TargetOverride),
	}
	if empty.HasOverrides() {
		t.Error("expected HasOverrides() = false for empty config")
	}

	withOverride := &Config{
		Run:   map[string]TargetOverride{"//foo:bar": {Args: []string{"--flag"}}},
		Build: make(map[string]TargetOverride),
		Test:  make(map[string]TargetOverride),
	}
	if !withOverride.HasOverrides() {
		t.Error("expected HasOverrides() = true for config with override")
	}
}
