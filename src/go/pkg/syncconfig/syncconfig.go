// Package syncconfig provides configuration management for tk sync.
//
// Configuration is stored in .turnkey/sync.toml and defines:
// - Dependency staleness rules (go.mod → go-deps.toml)
// - rules.star file staleness rules (coming soon)
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

	// Buck defines rules.star file staleness rules (future).
	// These track when source files change and need to regenerate rules.star files.
	Buck []BuckRule `toml:"buck"`

	// Wrappers defines tool wrapper rules for auto-sync.
	// These configure which native tools (go, cargo, uv) should trigger
	// sync operations when they modify dependency files.
	Wrappers []WrapperRule `toml:"wrappers"`

	// Rules configures automatic rules.star file synchronization.
	// When enabled, tk will update rules.star deps before build commands.
	Rules RulesConfig `toml:"rules"`
}

// RulesConfig configures automatic rules.star file synchronization.
type RulesConfig struct {
	// Enabled controls whether rules.star sync is active (default: false).
	// When true, tk will check/sync rules.star files before build commands.
	Enabled bool `toml:"enabled"`

	// AutoSync controls whether to automatically update stale rules.star files.
	// When true (default), stale files are updated. When false, tk only warns.
	AutoSync *bool `toml:"auto_sync,omitempty"`

	// Strict causes tk to fail if rules.star files would change.
	// This is useful for CI to ensure rules.star files are committed up-to-date.
	Strict bool `toml:"strict"`

	// Go contains Go-specific configuration for rules.star generation.
	Go GoRulesConfig `toml:"go"`
}

// GoRulesConfig contains Go-specific settings for rules.star generation.
type GoRulesConfig struct {
	// Enabled controls whether Go rules sync is active (default: true when parent enabled).
	Enabled *bool `toml:"enabled,omitempty"`

	// InternalPrefix is the Buck2 target prefix for internal packages.
	// Example: "//src/go" means imports from github.com/org/repo/src/go/pkg/foo
	// become //src/go/pkg/foo:foo
	InternalPrefix string `toml:"internal_prefix"`

	// ExternalCell is the Buck2 cell for external dependencies.
	// Example: "godeps" means external imports become godeps//vendor/path:target
	ExternalCell string `toml:"external_cell"`
}

// IsAutoSync returns whether auto-sync is enabled (defaults to true).
func (c *RulesConfig) IsAutoSync() bool {
	if c.AutoSync == nil {
		return true
	}
	return *c.AutoSync
}

// IsGoEnabled returns whether Go rules sync is enabled.
func (c *RulesConfig) IsGoEnabled() bool {
	if c.Go.Enabled == nil {
		return c.Enabled // Inherit from parent
	}
	return *c.Go.Enabled
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

// BuckRule defines a staleness rule for rules.star file generation.
// (Reserved for future implementation)
type BuckRule struct {
	// Name is a human-readable identifier for this rule.
	Name string `toml:"name"`

	// Patterns defines which source files trigger BUCK regeneration.
	Patterns []string `toml:"patterns"`

	// Generator is the command to regenerate rules.star files.
	Generator []string `toml:"generator"`

	// Enabled controls whether this rule is active (default: true).
	Enabled *bool `toml:"enabled,omitempty"`
}

// WrapperRule defines a tool wrapper configuration for auto-sync.
// When a wrapped tool modifies dependency files, the associated
// deps rule is triggered to regenerate the dependency declaration.
type WrapperRule struct {
	// Name is a human-readable identifier for this wrapper (e.g., "go", "cargo").
	Name string `toml:"name"`

	// Command is the underlying tool to wrap (e.g., "go", "cargo", "uv").
	Command string `toml:"command"`

	// MutatingSubcommands lists subcommands that may modify dependency files.
	// Examples: ["get", "mod"] for go, ["add", "remove", "update"] for cargo.
	MutatingSubcommands []string `toml:"mutating_subcommands"`

	// WatchFiles are the files to monitor for changes (relative to project root).
	// Examples: ["go.mod", "go.sum"], ["Cargo.toml", "Cargo.lock"].
	WatchFiles []string `toml:"watch_files"`

	// DepsRule is the name of the deps rule to trigger when files change.
	// This must match the Name field of a [[deps]] rule.
	DepsRule string `toml:"deps_rule"`

	// PostCommands are commands to run after the main command if files changed.
	// Each command is run in sequence before the sync operation.
	// Example: ["go mod tidy"] to clean up go.mod after go get.
	PostCommands []string `toml:"post_commands"`

	// Enabled controls whether this wrapper is active (default: true).
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

// IsEnabled returns whether the wrapper is enabled.
func (r *WrapperRule) IsEnabled() bool {
	if r.Enabled == nil {
		return true
	}
	return *r.Enabled
}

// IsMutatingSubcommand returns true if the given subcommand may modify dependency files.
func (r *WrapperRule) IsMutatingSubcommand(subcommand string) bool {
	for _, mut := range r.MutatingSubcommands {
		if subcommand == mut {
			return true
		}
	}
	return false
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

// EnabledWrapperRules returns only the enabled wrapper rules.
func (c *Config) EnabledWrapperRules() []WrapperRule {
	var rules []WrapperRule
	for _, r := range c.Wrappers {
		if r.IsEnabled() {
			rules = append(rules, r)
		}
	}
	return rules
}

// FindWrapper finds a wrapper rule by command name.
// Returns nil if no matching wrapper is found.
func (c *Config) FindWrapper(command string) *WrapperRule {
	for i := range c.Wrappers {
		if c.Wrappers[i].Command == command && c.Wrappers[i].IsEnabled() {
			return &c.Wrappers[i]
		}
	}
	return nil
}

// FindDepsRule finds a deps rule by name.
// Returns nil if no matching rule is found.
func (c *Config) FindDepsRule(name string) *DepsRule {
	for i := range c.Deps {
		if c.Deps[i].Name == name && c.Deps[i].IsEnabled() {
			return &c.Deps[i]
		}
	}
	return nil
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

	for i, r := range c.Wrappers {
		if r.Name == "" {
			return fmt.Errorf("wrapper rule %d: name is required", i)
		}
		if r.Command == "" {
			return fmt.Errorf("wrapper rule %q: command is required", r.Name)
		}
		if len(r.MutatingSubcommands) == 0 {
			return fmt.Errorf("wrapper rule %q: at least one mutating_subcommands is required", r.Name)
		}
		if len(r.WatchFiles) == 0 {
			return fmt.Errorf("wrapper rule %q: at least one watch_files is required", r.Name)
		}
		if r.DepsRule == "" {
			return fmt.Errorf("wrapper rule %q: deps_rule is required", r.Name)
		}
		// Verify the referenced deps rule exists
		if c.FindDepsRule(r.DepsRule) == nil {
			return fmt.Errorf("wrapper rule %q: deps_rule %q not found", r.Name, r.DepsRule)
		}
	}

	return nil
}
