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
}

// New creates a new Syncer with the given configuration.
func New(cfg *syncconfig.Config, root string) *Syncer {
	return &Syncer{
		Config: cfg,
		Root:   root,
		Output: os.Stderr,
	}
}

// SyncDeps checks and regenerates stale dependency files.
func (s *Syncer) SyncDeps() (*Result, error) {
	result := &Result{}

	rules := s.Config.EnabledDepsRules()
	for _, rule := range rules {
		result.Checked++

		stale, err := s.checkDepsRule(rule)
		if err != nil {
			result.Errors = append(result.Errors, fmt.Errorf("%s: %w", rule.Name, err))
			continue
		}

		if !stale {
			if !s.Quiet {
				s.printf("Checking %s... ok\n", rule.Target)
			}
			continue
		}

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

	return result, nil
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

// Check performs a staleness check without regenerating.
// Returns true if any targets are stale.
func (s *Syncer) Check() (*Result, bool, error) {
	result := &Result{}
	anyStale := false

	rules := s.Config.EnabledDepsRules()
	for _, rule := range rules {
		result.Checked++

		stale, err := s.checkDepsRule(rule)
		if err != nil {
			result.Errors = append(result.Errors, fmt.Errorf("%s: %w", rule.Name, err))
			continue
		}

		if stale {
			s.printf("%s: stale (%s newer than %s)\n", rule.Name, strings.Join(rule.Sources, ", "), rule.Target)
			anyStale = true
		} else if s.Verbose {
			s.printf("%s: ok\n", rule.Name)
		}
	}

	return result, anyStale, nil
}

// checkDepsRule checks if a single dependency rule's target is stale.
func (s *Syncer) checkDepsRule(rule syncconfig.DepsRule) (bool, error) {
	targetPath := filepath.Join(s.Root, rule.Target)

	// Resolve source paths with globs
	var sourcePaths []string
	for _, source := range rule.Sources {
		pattern := filepath.Join(s.Root, source)
		matches, err := filepath.Glob(pattern)
		if err != nil {
			return false, fmt.Errorf("invalid glob pattern %q: %w", source, err)
		}
		if len(matches) == 0 {
			// Source file might not exist yet, which means target is stale
			return true, nil
		}
		sourcePaths = append(sourcePaths, matches...)
	}

	return staleness.IsStale(sourcePaths, targetPath)
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
