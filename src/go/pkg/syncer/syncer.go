// Package syncer implements the sync logic for tk.
//
// Syncer uses the staleness package to check if targets are stale
// relative to their sources, and the syncconfig package to load
// configuration defining which files to check.
package syncer

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/staleness"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// Result represents the result of a sync operation.
type Result struct {
	// Checked is the number of rules checked.
	Checked int
	// Synced is the number of files regenerated.
	Synced int
	// Errors contains any errors encountered.
	Errors []error
}

// Syncer performs sync operations based on configuration.
type Syncer struct {
	// Config is the sync configuration.
	Config *syncconfig.Config
	// Root is the project root directory.
	Root string
	// Verbose enables verbose output.
	Verbose bool
	// Quiet suppresses non-error output (only shows errors and changes).
	Quiet bool
	// DryRun skips actual regeneration.
	DryRun bool
	// Output is where to write status messages.
	Output io.Writer
	// Only names the rules to sync or check. Empty means every enabled rule.
	Only []string
}

// New creates a new Syncer with the given configuration.
func New(cfg *syncconfig.Config, root string) *Syncer {
	return &Syncer{
		Config: cfg,
		Root:   root,
		Output: os.Stderr,
	}
}

// Load returns a Syncer for the project at root, from its
// .turnkey/sync.toml; a project without one has no rules.
func Load(root string) (*Syncer, error) {
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		return nil, fmt.Errorf("failed to load sync config: %w", err)
	}
	if err := cfg.Validate(); err != nil {
		return nil, fmt.Errorf("invalid sync config: %w", err)
	}
	return New(cfg, root), nil
}

// SyncDeps regenerates the rules' stale targets.
func (s *Syncer) SyncDeps() (*Result, error) {
	result, _, err := s.run(true)
	return result, err
}

// SyncRule regenerates a single dependency rule unconditionally.
// Unlike SyncDeps, it does not check staleness - it always regenerates.
// This is used by tw when it detects that dependency files have changed.
func (s *Syncer) SyncRule(rule syncconfig.DepsRule) error {
	if !rule.IsEnabled() {
		return nil
	}

	if !s.Quiet {
		s.printf("Syncing %s...\n", rule.Target)
	}

	if s.DryRun {
		s.printf("  Would regenerate %s (dry run)\n", rule.Target)
		return nil
	}

	if err := s.regenerate(rule); err != nil {
		return fmt.Errorf("%s: regeneration failed: %w", rule.Name, err)
	}

	if !s.Quiet {
		s.printf("  Regenerated %s\n", rule.Target)
	}
	return nil
}

// Check reports whether any rule's target is stale, without regenerating.
func (s *Syncer) Check() (*Result, bool, error) {
	return s.run(false)
}

// run checks each selected rule and, when regenerating, regenerates the
// stale ones. It returns whether any target was stale.
func (s *Syncer) run(regenerate bool) (*Result, bool, error) {
	result := &Result{}
	anyStale := false

	rules, err := s.selectedRules()
	if err != nil {
		return nil, false, err
	}
	for _, rule := range rules {
		result.Checked++

		state, err := s.freshness(rule)
		if err != nil {
			result.Errors = append(result.Errors, fmt.Errorf("%s: %w", rule.Name, err))
			continue
		}

		switch {
		case state == noSources:
			if s.Verbose {
				s.printf("%s: none of %s exists, nothing to generate %s from\n", rule.Name, strings.Join(rule.Sources, ", "), rule.Target)
			}
			continue
		case !regenerate && state == stale:
			s.printf("%s: stale (%s newer than %s)\n", rule.Name, strings.Join(rule.Sources, ", "), rule.Target)
			anyStale = true
			continue
		case !regenerate:
			if s.Verbose {
				s.printf("%s: ok\n", rule.Name)
			}
			continue
		case state == fresh:
			if !s.Quiet {
				s.printf("Checking %s... ok\n", rule.Target)
			}
			continue
		}

		anyStale = true
		if !s.Quiet {
			s.printf("Checking %s... stale\n", rule.Target)
		}
		if s.DryRun {
			s.printf("  Would regenerate %s (dry run)\n", rule.Target)
			result.Synced++
			continue
		}
		if err := s.regenerate(rule); err != nil {
			result.Errors = append(result.Errors, fmt.Errorf("%s: regeneration failed: %w", rule.Name, err))
			continue
		}
		s.printf("  Regenerated %s\n", rule.Target)
		result.Synced++
	}

	return result, anyStale, nil
}

// selectedRules returns the enabled rules named by Only, in config order,
// or every enabled rule when Only is empty. Rules run in config order, so a
// rule whose target is another's source comes first in sync.toml.
func (s *Syncer) selectedRules() ([]syncconfig.DepsRule, error) {
	rules := s.Config.EnabledDepsRules()
	if len(s.Only) == 0 {
		return rules, nil
	}
	var selected []syncconfig.DepsRule
	for _, name := range s.Only {
		if !slices.ContainsFunc(rules, func(r syncconfig.DepsRule) bool { return r.Name == name }) {
			return nil, fmt.Errorf("no enabled deps rule named %q", name)
		}
	}
	for _, rule := range rules {
		if slices.Contains(s.Only, rule.Name) {
			selected = append(selected, rule)
		}
	}
	return selected, nil
}

// freshness is where a rule's target stands against its sources.
type freshness int

const (
	fresh freshness = iota
	stale
	// noSources: none of the rule's sources exists (Go enabled in a project
	// without a go.mod), so there is nothing to generate the target from.
	noSources
)

// freshness compares a rule's target with the sources that exist; a missing
// source (a go.sum in a module without dependencies) doesn't count.
func (s *Syncer) freshness(rule syncconfig.DepsRule) (freshness, error) {
	sources := make([]string, len(rule.Sources))
	for i, source := range rule.Sources {
		sources[i] = filepath.Join(s.Root, source)
	}
	result, err := staleness.Check(sources, filepath.Join(s.Root, rule.Target))
	if err != nil {
		return fresh, err
	}
	switch {
	case result.NewestSource == nil:
		return noSources, nil
	case result.Stale:
		return stale, nil
	default:
		return fresh, nil
	}
}

// regenerate runs the generator command for a rule.
func (s *Syncer) regenerate(rule syncconfig.DepsRule) error {
	if len(rule.Generator) == 0 {
		return fmt.Errorf("no generator command specified")
	}

	targetPath := filepath.Join(s.Root, rule.Target)

	// Create parent directories if needed
	if err := os.MkdirAll(filepath.Dir(targetPath), 0755); err != nil {
		return fmt.Errorf("failed to create target directory: %w", err)
	}

	// Run the generator command
	cmd := exec.Command(rule.Generator[0], rule.Generator[1:]...)
	cmd.Dir = s.Root

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if s.Verbose {
		s.printf("  Running: %s\n", strings.Join(rule.Generator, " "))
	}

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("generator failed: %w\n%s", err, stderr.String())
	}

	// Write stdout to target file
	if err := os.WriteFile(targetPath, stdout.Bytes(), 0644); err != nil {
		return fmt.Errorf("failed to write target: %w", err)
	}

	return nil
}

func (s *Syncer) printf(format string, args ...interface{}) {
	if s.Output != nil {
		_, _ = fmt.Fprintf(s.Output, format, args...)
	}
}
