// Package syncconfig provides configuration management for tk sync.
//
// Configuration is stored in .turnkey/sync.toml and defines:
// - Dependency staleness rules (go.mod → go-deps.toml)
// - BUCK file staleness rules (coming soon)
//
// Example config:
//
//	[[deps]]
//	name = "go"
//	sources = ["go.mod", "go.sum"]
//	target = "go-deps.toml"
//	generator = ["godeps-gen", "--go-mod", "go.mod", "--go-sum", "go.sum"]
//
//	[[deps]]
//	name = "rust"
//	sources = ["Cargo.toml", "Cargo.lock"]
//	target = "rust-deps.toml"
//	generator = ["cargo-deps-gen"]
package syncconfig

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/pelletier/go-toml/v2"
)

// DefaultConfigPath is the default location for the sync config file.
const DefaultConfigPath = ".turnkey/sync.toml"

// Config represents the entire sync configuration.
type Config struct {
	// Deps defines dependency staleness rules.
	// These track when dependency files (go.mod, Cargo.toml) change
	// and need to regenerate dependency declarations.
	Deps []DepsRule `toml:"deps"`

	// Buck defines BUCK file staleness rules (future).
	// These track when source files change and need to regenerate BUCK files.
	Buck []BuckRule `toml:"buck"`
}

// DepsRule defines a staleness rule for dependency generation.
type DepsRule struct {
	// Name is a human-readable identifier for this rule.
	Name string `toml:"name"`

	// Sources are the files to watch for changes (globs supported).
	// Examples: ["go.mod", "go.sum"], ["**/Cargo.toml", "**/Cargo.lock"]
	Sources []string `toml:"sources"`

	// Target is the file to generate when sources change.
	Target string `toml:"target"`

	// Generator is the command to run to regenerate the target.
	// The command is executed from the project root.
	Generator []string `toml:"generator"`

	// Enabled controls whether this rule is active (default: true).
	Enabled *bool `toml:"enabled,omitempty"`
}

// BuckRule defines a staleness rule for BUCK file generation.
// (Reserved for future implementation)
type BuckRule struct {
	// Name is a human-readable identifier for this rule.
	Name string `toml:"name"`

	// Patterns defines which source files trigger BUCK regeneration.
	Patterns []string `toml:"patterns"`

	// Generator is the command to regenerate BUCK files.
	Generator []string `toml:"generator"`

	// Enabled controls whether this rule is active (default: true).
	Enabled *bool `toml:"enabled,omitempty"`
}

// IsEnabled returns whether the rule is enabled.
func (r *DepsRule) IsEnabled() bool {
	if r.Enabled == nil {
		return true
	}
	return *r.Enabled
}

// IsEnabled returns whether the rule is enabled.
func (r *BuckRule) IsEnabled() bool {
	if r.Enabled == nil {
		return true
	}
	return *r.Enabled
}

// Load reads the config file from the given path.
func Load(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read config: %w", err)
	}

	return Parse(data)
}

// LoadDefault loads the config from the default path (.turnkey/sync.toml).
// If the file doesn't exist, returns an empty config (not an error).
func LoadDefault() (*Config, error) {
	return LoadDefaultFrom(".")
}

// LoadDefaultFrom loads the config from the default path relative to root.
// If the file doesn't exist, returns an empty config (not an error).
func LoadDefaultFrom(root string) (*Config, error) {
	path := filepath.Join(root, DefaultConfigPath)

	if _, err := os.Stat(path); os.IsNotExist(err) {
		return &Config{}, nil
	}

	return Load(path)
}

// Parse parses the config from TOML data.
func Parse(data []byte) (*Config, error) {
	var cfg Config
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("failed to parse config: %w", err)
	}

	return &cfg, nil
}

// EnabledDepsRules returns only the enabled dependency rules.
func (c *Config) EnabledDepsRules() []DepsRule {
	var rules []DepsRule
	for _, r := range c.Deps {
		if r.IsEnabled() {
			rules = append(rules, r)
		}
	}
	return rules
}

// EnabledBuckRules returns only the enabled BUCK rules.
func (c *Config) EnabledBuckRules() []BuckRule {
	var rules []BuckRule
	for _, r := range c.Buck {
		if r.IsEnabled() {
			rules = append(rules, r)
		}
	}
	return rules
}

// Validate checks the config for common errors.
func (c *Config) Validate() error {
	for i, r := range c.Deps {
		if r.Name == "" {
			return fmt.Errorf("deps rule %d: name is required", i)
		}
		if len(r.Sources) == 0 {
			return fmt.Errorf("deps rule %q: at least one source is required", r.Name)
		}
		if r.Target == "" {
			return fmt.Errorf("deps rule %q: target is required", r.Name)
		}
		if len(r.Generator) == 0 {
			return fmt.Errorf("deps rule %q: generator command is required", r.Name)
		}
	}

	for i, r := range c.Buck {
		if r.Name == "" {
			return fmt.Errorf("buck rule %d: name is required", i)
		}
		if len(r.Patterns) == 0 {
			return fmt.Errorf("buck rule %q: at least one pattern is required", r.Name)
		}
		if len(r.Generator) == 0 {
			return fmt.Errorf("buck rule %q: generator command is required", r.Name)
		}
	}

	return nil
}
