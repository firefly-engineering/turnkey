package buckgen

import (
	"encoding/json"
	"os"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// Config represents buckgen configuration
type Config struct {
	Buck BuckConfig `json:"buck"`

	// Conditions are the configurations a package's deps are resolved
	// for: the platforms turnkey builds for (buck2.platforms) and the
	// allowed Go build tags (buck2.go.allowedBuildTags).
	Conditions ConditionsConfig `json:"conditions"`

	// GoVersion is the Go toolchain's version (e.g. "1.24"): files
	// constrained to a later release are left out.
	GoVersion string `json:"go_version"`
}

type BuckConfig struct {
	Preambule             string `json:"preambule"` // Note: intentional spelling
	GoLibraryRule         string `json:"go_library_rule"`
	DepsTargetLabelPrefix string `json:"deps_target_label_prefix"`
	DepsAttr              string `json:"deps_attr"`
	BuildfileName         string `json:"buildfile_name"`
}

// ConditionsConfig names the configurations as turnkey's Nix does
// (nix/buck2/platforms.nix).
type ConditionsConfig struct {
	// Settings is the package of the combined config_settings.
	Settings string `json:"settings"`

	// Platforms are the platforms, in Buck2's names.
	Platforms []conditions.Platform `json:"platforms"`

	// GoTags are the build tags that may vary per configuration.
	GoTags []string `json:"go_tags"`
}

// LoadConfig loads config from buckgen.json
func LoadConfig(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}
