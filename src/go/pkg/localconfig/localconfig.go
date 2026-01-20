// Package localconfig provides local target override configuration for tk.
//
// Configuration is stored in .turnkey/local.toml (not committed to git) and allows
// per-developer customization of target arguments. This is useful when different
// developers need different local settings (e.g., different network addresses,
// debug flags, local ports).
//
// Example config:
//
//	[run."//docs/user-manual"]
//	args = ["-n", "100.64.25.26"]
//
//	[build."//some:target"]
//	args = ["--config=debug"]
//
//	[test."//src/pkg/..."]
//	args = ["--test-arg=foo"]
package localconfig

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// DefaultConfigPath is the default location for the local config file.
const DefaultConfigPath = ".turnkey/local.toml"

// Config represents the local override configuration.
type Config struct {
	// Run contains per-target argument overrides for `tk run`.
	Run map[string]TargetOverride `toml:"run"`

	// Build contains per-target argument overrides for `tk build`.
	Build map[string]TargetOverride `toml:"build"`

	// Test contains per-target argument overrides for `tk test`.
	Test map[string]TargetOverride `toml:"test"`
}

// TargetOverride defines the overrides for a specific target.
type TargetOverride struct {
	// Args are additional arguments to append after `--` for this target.
	Args []string `toml:"args"`
}

// Load reads the config file from the given path.
func Load(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read local config: %w", err)
	}

	return Parse(data)
}

// LoadDefault loads the config from the default path (.turnkey/local.toml).
// If the file doesn't exist, returns an empty config (not an error).
func LoadDefault() (*Config, error) {
	return LoadDefaultFrom(".")
}

// LoadDefaultFrom loads the config from the default path relative to root.
// If the file doesn't exist, returns an empty config (not an error).
func LoadDefaultFrom(root string) (*Config, error) {
	path := filepath.Join(root, DefaultConfigPath)

	if _, err := os.Stat(path); os.IsNotExist(err) {
		return &Config{
			Run:   make(map[string]TargetOverride),
			Build: make(map[string]TargetOverride),
			Test:  make(map[string]TargetOverride),
		}, nil
	}

	return Load(path)
}

// Parse parses the config from TOML data.
func Parse(data []byte) (*Config, error) {
	var cfg Config
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("failed to parse local config: %w", err)
	}

	// Initialize empty maps if nil
	if cfg.Run == nil {
		cfg.Run = make(map[string]TargetOverride)
	}
	if cfg.Build == nil {
		cfg.Build = make(map[string]TargetOverride)
	}
	if cfg.Test == nil {
		cfg.Test = make(map[string]TargetOverride)
	}

	return &cfg, nil
}

// GetOverride returns the override for a specific command and target.
// The target can be an exact match or a pattern match (prefix match for "//pkg/...").
// Returns nil if no matching override is found.
func (c *Config) GetOverride(command, target string) *TargetOverride {
	var overrides map[string]TargetOverride

	switch command {
	case "run":
		overrides = c.Run
	case "build":
		overrides = c.Build
	case "test":
		overrides = c.Test
	default:
		return nil
	}

	// Try exact match first
	if override, ok := overrides[target]; ok {
		return &override
	}

	// Try pattern match (for "//pkg/..." style patterns)
	for pattern, override := range overrides {
		if matchTarget(pattern, target) {
			o := override // Copy to avoid returning reference to map value
			return &o
		}
	}

	return nil
}

// matchTarget checks if a target matches a pattern.
// Patterns ending with "..." match any target with that prefix.
// For example:
//   - "//foo/..." matches "//foo:bar", "//foo/sub:baz"
//   - "//..." matches any target
func matchTarget(pattern, target string) bool {
	if pattern == target {
		return true
	}

	// Handle "//pkg/..." patterns
	if strings.HasSuffix(pattern, "...") {
		prefix := strings.TrimSuffix(pattern, "...")
		// "//foo/..." should match both "//foo:bar" and "//foo/sub:bar"
		// So we need to check if target starts with prefix, or starts with
		// the prefix with the trailing / removed (for same-package targets)
		if strings.HasPrefix(target, prefix) {
			return true
		}
		// Also match "//foo:bar" when pattern is "//foo/..."
		// The prefix would be "//foo/" so we check "//foo"
		if len(prefix) > 0 && prefix[len(prefix)-1] == '/' {
			packagePath := prefix[:len(prefix)-1]
			if strings.HasPrefix(target, packagePath+":") {
				return true
			}
		}
	}

	return false
}

// HasOverrides returns true if there are any overrides configured.
func (c *Config) HasOverrides() bool {
	return len(c.Run) > 0 || len(c.Build) > 0 || len(c.Test) > 0
}
