package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/pelletier/go-toml/v2"
)

// RustConfig holds Rust-specific configuration.
type RustConfig struct {
	// WorkspaceRoot is the Cargo workspace root directory.
	WorkspaceRoot string

	// ExternalCell is the Buck2 cell for external deps (e.g., "rustdeps").
	ExternalCell string

	// DepsFile is the path to rust-deps.toml.
	DepsFile string

	// ExternalDeps maps crate names to their entries from rust-deps.toml.
	ExternalDeps map[string]bool

	// WorkspacePackages maps the package names of the workspace's members
	// to their directories, relative to the project root.
	WorkspacePackages map[string]string

	// workspace is the workspace root, whose [workspace.dependencies]
	// workspace = true entries inherit from.
	workspace *cargoWorkspace
}

// rustRules are the Rust rule kinds. A Rust target can ask for features
// in Cargo's terms.
var rustRules = map[string]Rule{
	"rust_library": {Kind: Library, DepsAttribute: "deps", Variant: cargoVariantAttributes},
	"rust_binary":  {Kind: Binary, DepsAttribute: "deps", Variant: cargoVariantAttributes},
	"rust_test":    {Kind: Test, DepsAttribute: "deps", Variant: cargoVariantAttributes},
}

// rustLanguage resolves a Rust crate's deps from its Cargo.toml.
type rustLanguage struct {
	projectRoot string
	cfg         *RustConfig

	// rules caches the members' parsed rules.star, by member directory
	rules map[string]*starlark.File
}

func newRustLanguage(mcfg Config, lang syncconfig.Language) Language {
	cfg, _ := detectRustConfig(mcfg.ProjectRoot, lang)
	return &rustLanguage{projectRoot: mcfg.ProjectRoot, cfg: cfg}
}

func (l *rustLanguage) Name() string { return "rust" }

func (l *rustLanguage) Rule(rule string) (Rule, bool) {
	r, ok := rustRules[rule]
	return r, ok
}

func (l *rustLanguage) SourcePatterns() []string { return []string{"*.rs", "Cargo.toml"} }

// Dimensions: a crate's target-specific tables, and its dependencies'
// member targets' variants, depend on the platform.
func (l *rustLanguage) Dimensions(string) ([]string, error) {
	return []string{conditions.OS, conditions.CPU}, nil
}

func (l *rustLanguage) ResolveDeps(crateDir string, req Request) (PackageMapping, error) {
	mapping, err := l.resolveCrate(crateDir, req)
	if err != nil {
		return mapping, fmt.Errorf("reading Cargo.toml: %w", err)
	}
	return mapping, nil
}

// detectRustConfig auto-detects Rust configuration from the project, with
// the language's cell and deps file.
func detectRustConfig(projectRoot string, lang syncconfig.Language) (*RustConfig, error) {
	cfg := &RustConfig{
		ExternalCell:      lang.Cell,
		ExternalDeps:      make(map[string]bool),
		WorkspacePackages: make(map[string]string),
	}

	// Check for Cargo.toml
	cargoPath := filepath.Join(projectRoot, "Cargo.toml")
	if _, err := os.Stat(cargoPath); err != nil {
		return nil, fmt.Errorf("no Cargo.toml found")
	}
	cfg.WorkspaceRoot = projectRoot

	// A root Cargo.toml without [workspace] is a single crate: nothing to
	// inherit from and no members.
	if ws, members, err := loadCargoWorkspace(projectRoot, projectRoot); err == nil {
		cfg.workspace = ws
		cfg.WorkspacePackages = members
	}

	// Load rust-deps.toml
	depsPath := filepath.Join(projectRoot, lang.DepsFile)
	if deps, err := loadRustDeps(depsPath); err == nil {
		cfg.DepsFile = depsPath
		cfg.ExternalDeps = deps
	}

	return cfg, nil
}

// loadRustDeps loads crate names from rust-deps.toml.
func loadRustDeps(path string) (map[string]bool, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var depsFile struct {
		Deps map[string]interface{} `toml:"deps"`
	}
	if err := toml.Unmarshal(content, &depsFile); err != nil {
		return nil, err
	}

	result := make(map[string]bool)
	for dep := range depsFile.Deps {
		// rust-deps.toml uses "crate@version" as keys, extract just the crate name
		if idx := strings.Index(dep, "@"); idx > 0 {
			result[dep[:idx]] = true
		} else {
			result[dep] = true
		}
	}
	return result, nil
}
