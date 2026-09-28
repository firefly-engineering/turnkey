// Package syncconfig provides configuration management for tk sync.
//
// Configuration is stored in .turnkey/sync.toml and defines:
// - Dependency staleness rules (go.mod → go-deps.toml)
// - Tool wrapper rules, for tw
// - Rules sync's settings, conditions and languages
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
//
//	[[languages]]
//	name = "go"
//	cell = "godeps"
//	deps_file = "go-deps.toml"
//
//	[conditions]
//	settings = "toolchains//conditions"
//
//	[[conditions.platforms]]
//	os = "linux"
//	cpu = "x86_64"
package syncconfig

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
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

	// Wrappers defines tool wrapper rules for auto-sync.
	// These configure which native tools (go, cargo, uv) should trigger
	// sync operations when they modify dependency files.
	Wrappers []WrapperRule `toml:"wrappers"`

	// Rules configures automatic rules.star file synchronization.
	// When enabled, tk will update rules.star deps before build commands.
	Rules RulesConfig `toml:"rules"`

	// Conditions are the build configurations rules sync evaluates, for
	// deps that differ between them.
	Conditions ConditionsConfig `toml:"conditions"`

	// Languages are the languages turnkey manages dependencies for, one per
	// language record (turnkey's nix/buck2/languages.nix).
	Languages []Language `toml:"languages"`
}

// Language is what rules sync knows of a language from its record: where
// the language's external deps are.
type Language struct {
	// Name is the record's name, e.g. "go" or "javascript".
	Name string `toml:"name"`

	// Cell is the Buck2 cell holding the language's external deps
	// (e.g. "godeps").
	Cell string `toml:"cell"`

	// DepsFile is the deps file the cell is built from, relative to the
	// project root (e.g. "go-deps.toml").
	DepsFile string `toml:"deps_file"`
}

// ConditionsConfig describes the build configurations rules sync
// evaluates, from turnkey's Nix options.
type ConditionsConfig struct {
	// Settings is the Buck2 package holding turnkey's combined
	// config_settings, one per platform, named "<os>-<cpu>"
	// (e.g. "toolchains//conditions").
	Settings string `toml:"settings"`

	// Platforms are the platforms turnkey builds for (buck2.platforms).
	Platforms []conditions.Platform `toml:"platforms"`

	// GoTags are the Go build tags allowed to vary per configuration
	// (buck2.go.allowedBuildTags, .buckconfig's go.allowed_build_tags).
	GoTags []string `toml:"go_tags"`
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
}

// IsAutoSync returns whether auto-sync is enabled (defaults to true).
func (c *RulesConfig) IsAutoSync() bool {
	if c.AutoSync == nil {
		return true
	}
	return *c.AutoSync
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

	// TargetSources, when set, is a top-level key of the target (a TOML
	// file) whose array lists more sources, relative to the project root:
	// files only the generator can find, such as a Cargo workspace's member
	// manifests. A target without that key is stale.
	TargetSources string `toml:"target_sources,omitempty"`

	// Generator is the command to run to regenerate the target.
	// The command is executed from the project root.
	Generator []string `toml:"generator"`
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

// FindRoot returns the project root dir is in: the nearest of dir and its
// ancestors holding a .buckconfig or a .turnkey/sync.toml. ok is false
// when there is none, dir being outside any project; what to do then is
// the caller's (tk runs from dir, tw runs the tool untouched).
//
// Either file marks a root: tk needs .buckconfig to run buck2, tw needs
// sync.toml to know what to sync, and a turnkey shell writes both at the
// project's top, so the nearest of either is the same directory for both.
func FindRoot(dir string) (root string, ok bool) {
	dir = filepath.Clean(dir)
	for {
		for _, marker := range []string{".buckconfig", DefaultConfigPath} {
			if _, err := os.Stat(filepath.Join(dir, marker)); err == nil {
				return dir, true
			}
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", false
		}
		dir = parent
	}
}

// Parse parses the config from TOML data.
func Parse(data []byte) (*Config, error) {
	var cfg Config
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("failed to parse config: %w", err)
	}

	return &cfg, nil
}

// FindWrapper finds a wrapper rule by command name.
// Returns nil if no matching wrapper is found.
func (c *Config) FindWrapper(command string) *WrapperRule {
	for i := range c.Wrappers {
		if c.Wrappers[i].Command == command {
			return &c.Wrappers[i]
		}
	}
	return nil
}

// FindDepsRule finds a deps rule by name.
// Returns nil if no matching rule is found.
func (c *Config) FindDepsRule(name string) *DepsRule {
	for i := range c.Deps {
		if c.Deps[i].Name == name {
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

	seen := make(map[string]bool)
	for i, l := range c.Languages {
		if l.Name == "" {
			return fmt.Errorf("language %d: name is required", i)
		}
		if seen[l.Name] {
			return fmt.Errorf("language %q: listed twice", l.Name)
		}
		seen[l.Name] = true
		if l.Cell == "" {
			return fmt.Errorf("language %q: cell is required", l.Name)
		}
		if l.DepsFile == "" {
			return fmt.Errorf("language %q: deps_file is required", l.Name)
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

// Space returns the configuration space of the configured platforms.
func (c ConditionsConfig) Space() conditions.Space {
	return conditions.NewSpace(c.Platforms, c.Settings)
}
