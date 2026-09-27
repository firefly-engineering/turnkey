// Package rulessync orchestrates rules.star synchronization:
// - the mapper's language plug-ins classify rule kinds and resolve deps
// - the starlark object model reads and writes rules.star
package rulessync

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditional"
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/mapper"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// Config holds syncer configuration.
type Config struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// DryRun if true, doesn't write changes.
	DryRun bool

	// Verbose enables verbose output.
	Verbose bool

	// Force if true, syncs all files even if they appear up-to-date.
	// When false, uses mtime-based staleness detection to skip files.
	Force bool

	// Sync is the project's sync configuration: the languages sync
	// resolves deps for and the build configurations it evaluates. When
	// nil, it is read from the project's .turnkey/sync.toml.
	Sync *syncconfig.Config
}

// Syncer orchestrates rules.star synchronization.
type Syncer struct {
	config Config
	mapper *mapper.Mapper

	// space holds the configurations every target's deps are resolved
	// for, so that what sync writes doesn't depend on the host.
	space conditions.Space
}

// NewSyncer creates a new Syncer, with the plug-ins of the configured
// languages.
func NewSyncer(cfg Config) (*Syncer, error) {
	if err := cfg.loadSync(); err != nil {
		return nil, err
	}
	// Every sync.toml turnkey writes lists its languages: one without them
	// is missing or predates them, and the turnkey shell regenerates it.
	// Syncing no language would pass every check.
	if len(cfg.Sync.Languages) == 0 {
		return nil, fmt.Errorf("%s lists no [[languages]]: re-enter the turnkey shell to regenerate it", syncconfig.DefaultConfigPath)
	}
	m, err := mapper.New(mapper.Config{
		ProjectRoot: cfg.ProjectRoot,
		Languages:   cfg.Sync.Languages,
		Conditions:  cfg.Sync.Conditions,
	})
	if err != nil {
		return nil, fmt.Errorf("creating mapper: %w", err)
	}

	return newSyncer(cfg, m)
}

// newSyncer creates a Syncer with the given mapper.
func newSyncer(cfg Config, m *mapper.Mapper) (*Syncer, error) {
	if err := cfg.loadSync(); err != nil {
		return nil, err
	}

	return &Syncer{
		config: cfg,
		mapper: m,
		space:  cfg.Sync.Conditions.Space(),
	}, nil
}

// loadSync reads the project's sync configuration, unless it was given.
func (cfg *Config) loadSync() error {
	if cfg.Sync != nil {
		return nil
	}
	syncCfg, err := syncconfig.LoadDefaultFrom(cfg.ProjectRoot)
	if err != nil {
		return fmt.Errorf("reading sync config: %w", err)
	}
	if err := syncCfg.Validate(); err != nil {
		return fmt.Errorf("invalid sync config: %w", err)
	}
	cfg.Sync = syncCfg
	return nil
}

// SyncResult contains the result of syncing a single rules.star file.
type SyncResult struct {
	// Path is the path to the rules.star file.
	Path string

	// Updated is true if the file was modified.
	Updated bool

	// Skipped is true if the file was skipped due to staleness check.
	Skipped bool

	// Changes lists, per target whose deps changed, what was added and
	// removed, in the order the targets appear in the file.
	Changes []TargetChange

	// OptedOut lists the targets a "# turnkey:no-sync" comment opts out.
	OptedOut []string

	// Unreadable lists the targets sync skipped because their deps aren't
	// a list of labels, and nothing opts them out.
	Unreadable []UnreadableTarget

	// Errors contains any errors encountered.
	Errors []string
}

// UnreadableTarget is a target whose deps attribute sync can't read.
type UnreadableTarget struct {
	// Target is the target's name.
	Target string

	// Attribute is the deps attribute that isn't a list of labels.
	Attribute string
}

// TargetChange records how sync changed one target's deps, or another
// attribute it owns.
type TargetChange struct {
	// Target is the target's name.
	Target string

	// Attribute is the attribute that changed, when it isn't the deps.
	Attribute string

	// Added lists dependencies that were added.
	Added []string

	// Removed lists dependencies that were removed.
	Removed []string

	// Kept lists dependencies sync would have removed but kept because
	// the target has unmapped imports (listed in Unmapped).
	Kept []string

	// Unmapped lists the imports that made sync keep the deps in Kept.
	Unmapped []string
}

// SyncDirectory syncs all rules.star files in a directory tree.
func (s *Syncer) SyncDirectory(dir string) ([]SyncResult, error) {
	var results []SyncResult

	// If not forcing, use git status to find only directories with changes
	if !s.config.Force {
		changedDirs, err := s.getChangedDirectories(dir)
		if err != nil {
			// Fall back to full walk on git error
			if s.config.Verbose {
				fmt.Fprintf(os.Stderr, "  git status failed, falling back to full walk: %v\n", err)
			}
		} else if len(changedDirs) == 0 {
			// No changes detected by git
			return results, nil
		} else {
			// Only process rules.star files in changed directories
			for pkgDir := range changedDirs {
				rulesPath := filepath.Join(pkgDir, "rules.star")
				if _, err := os.Stat(rulesPath); err != nil {
					continue // No rules.star in this directory
				}

				result, err := s.SyncFile(rulesPath)
				if err != nil {
					results = append(results, SyncResult{
						Path:   rulesPath,
						Errors: []string{err.Error()},
					})
				} else {
					results = append(results, *result)
				}
			}
			return results, nil
		}
	}

	// Force mode or git fallback: walk entire tree
	err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip vendor and hidden directories
		if info.IsDir() {
			name := info.Name()
			if name == "vendor" || name == "testdata" || strings.HasPrefix(name, ".") {
				return filepath.SkipDir
			}
			return nil
		}

		// Only process rules.star files
		if info.Name() != "rules.star" {
			return nil
		}

		result, err := s.SyncFile(path)
		if err != nil {
			results = append(results, SyncResult{
				Path:   path,
				Errors: []string{err.Error()},
			})
		} else {
			results = append(results, *result)
		}

		return nil
	})

	return results, err
}

// SyncFile syncs a single rules.star file.
func (s *Syncer) SyncFile(rulesPath string) (*SyncResult, error) {
	result := &SyncResult{Path: rulesPath}

	// Determine package directory
	pkgDir := filepath.Dir(rulesPath)

	// Parse the rules.star file to detect language
	f, err := starlark.ParseFile(rulesPath)
	if err != nil {
		return nil, fmt.Errorf("parsing rules.star: %w", err)
	}

	// Detect language from rules.star content
	lang := s.detectLanguage(f)
	if lang == nil {
		// Can't determine language, skip
		return result, nil
	}

	// Check staleness before running extractor (unless Force mode)
	if !s.config.Force {
		stale, err := s.isStale(rulesPath, pkgDir, lang.SourcePatterns())
		if err != nil {
			// On error, assume stale to be safe
			if s.config.Verbose {
				fmt.Fprintf(os.Stderr, "  staleness check failed for %s: %v\n", rulesPath, err)
			}
		} else if !stale {
			result.Skipped = true
			return result, nil
		}
	}

	pkg, err := mapper.OpenPackage(lang, s.config.ProjectRoot, pkgDir, s.space)
	if err != nil {
		result.Errors = append(result.Errors, err.Error())
		return result, nil
	}
	space := pkg.Space()
	if err := pkg.ResolveAll(); err != nil {
		result.Errors = append(result.Errors, err.Error())
		return result, nil
	}

	// Apply changes to targets
	modified := false

	for _, target := range f.Targets {
		rule, ok := lang.Rule(target.Rule)
		if !ok || rule.Kind == mapper.NotSynced {
			continue
		}
		attr := rule.DepsAttribute
		if target.NoSync {
			result.OptedOut = append(result.OptedOut, target.Name)
			continue
		}
		old, err := conditional.ReadLabels(target, attr, space)
		if err != nil {
			result.Unreadable = append(result.Unreadable, UnreadableTarget{Target: target.Name, Attribute: attr})
			continue
		}
		want, badAttr, ok := pkg.Target(target, rule)
		if !ok {
			result.Unreadable = append(result.Unreadable, UnreadableTarget{Target: target.Name, Attribute: badAttr})
			continue
		}

		changed, err := result.applyConditional(target, attr, space, old, func(config conditions.Configuration) (mapper.Want, error) {
			return want.Deps(config, old(config))
		})
		if err != nil {
			result.Errors = append(result.Errors, err.Error())
			return result, nil
		}
		if changed {
			modified = true
		}

		// The other attributes the language owns, e.g. Rust's features
		owned, err := want.Owned()
		if err != nil {
			result.Errors = append(result.Errors, err.Error())
			return result, nil
		}
		for _, name := range owned {
			oldValue, err := conditional.ReadLabels(target, name, space)
			if err != nil {
				result.Unreadable = append(result.Unreadable, UnreadableTarget{Target: target.Name, Attribute: name})
				continue
			}
			changed, err := result.applyOwned(target, name, space, oldValue, func(config conditions.Configuration) ([]string, error) {
				return want.Attr(config, name)
			})
			if err != nil {
				result.Errors = append(result.Errors, err.Error())
				return result, nil
			}
			if changed {
				modified = true
			}
		}
	}
	result.Errors = append(result.Errors, pkg.Messages()...)

	// Write if modified
	if modified && !s.config.DryRun {
		output := f.Write()
		if err := os.WriteFile(rulesPath, output, 0644); err != nil {
			return nil, fmt.Errorf("writing rules.star: %w", err)
		}
	}

	result.Updated = modified
	return result, nil
}

// getChangedDirectories uses git status to find directories with changed source files.
// Returns a map of absolute directory paths that have uncommitted changes.
func (s *Syncer) getChangedDirectories(dir string) (map[string]bool, error) {
	cmd := exec.Command("git", "status", "--porcelain", "-uall")
	cmd.Dir = s.config.ProjectRoot

	output, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("git status failed: %w", err)
	}

	changedDirs := make(map[string]bool)

	var sourcePatterns []string
	for _, lang := range s.mapper.Languages() {
		sourcePatterns = append(sourcePatterns, lang.SourcePatterns()...)
	}

	for _, line := range strings.Split(string(output), "\n") {
		if len(line) < 4 {
			continue
		}

		// Parse git status output: "XY filename" or "XY orig -> renamed"
		filePath := strings.TrimSpace(line[3:])
		if idx := strings.Index(filePath, " -> "); idx >= 0 {
			filePath = filePath[idx+4:] // Use the destination of rename
		}

		// Get absolute path
		absPath := filepath.Join(s.config.ProjectRoot, filePath)

		// Check if this is a source file of any language
		if !matchesAny(filepath.Base(filePath), sourcePatterns) {
			continue
		}

		// Get the directory containing this file
		fileDir := filepath.Dir(absPath)

		// Walk up the directory tree to find rules.star
		// A change in src/cmd/tk/main.go should trigger src/cmd/tk/rules.star
		for d := fileDir; strings.HasPrefix(d, s.config.ProjectRoot); d = filepath.Dir(d) {
			rulesPath := filepath.Join(d, "rules.star")
			if _, err := os.Stat(rulesPath); err == nil {
				changedDirs[d] = true
				break
			}
			// Don't go above the search directory
			if d == dir || d == s.config.ProjectRoot {
				break
			}
		}
	}

	return changedDirs, nil
}

// isStale checks if rules.star needs updating based on source file mtimes.
// Returns true if any file matching patterns is newer than rules.star.
func (s *Syncer) isStale(rulesPath, pkgDir string, patterns []string) (bool, error) {
	// Get rules.star mtime
	rulesInfo, err := os.Stat(rulesPath)
	if err != nil {
		return true, err // If we can't stat rules.star, assume stale
	}
	rulesMtime := rulesInfo.ModTime()

	if len(patterns) == 0 {
		return true, nil // No known sources, assume stale
	}

	// Walk the directory and check mtimes
	var newerCount int
	err = filepath.Walk(pkgDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip hidden directories and common non-source directories
		if info.IsDir() {
			name := info.Name()
			if name == "vendor" || name == "node_modules" || name == "testdata" ||
				name == "__pycache__" || name == ".venv" || name == "target" ||
				strings.HasPrefix(name, ".") {
				return filepath.SkipDir
			}
			return nil
		}

		if matchesAny(info.Name(), patterns) && info.ModTime().After(rulesMtime) {
			newerCount++
			// Found a newer file, we're done
			return filepath.SkipAll
		}
		return nil
	})

	if err != nil && err != filepath.SkipAll {
		return true, err
	}

	return newerCount > 0, nil
}

// detectLanguage returns the plug-in of the first target whose rule kind a
// language owns, or nil.
func (s *Syncer) detectLanguage(f *starlark.File) mapper.Language {
	for _, target := range f.Targets {
		if lang, _ := s.mapper.RuleLanguage(target.Rule); lang != nil {
			return lang
		}
	}
	return nil
}

// matchesAny reports whether a file name matches any of patterns.
func matchesAny(name string, patterns []string) bool {
	for _, pattern := range patterns {
		if matched, _ := filepath.Match(pattern, name); matched {
			return true
		}
	}
	return false
}

// mergeWithPreserved merges new deps with preserved deps from old list.
// Preserves:
// - Local target deps (starting with ":") - these are manual same-package deps
// - Deps outside the auto-managed section of a list with markers (preserved)
func mergeWithPreserved(oldDeps, newDeps, preserved []string) []string {
	isPreserved := make(map[string]bool, len(preserved))
	for _, d := range preserved {
		isPreserved[d] = true
	}

	// Build set of new deps for deduplication
	seen := make(map[string]bool)
	for _, d := range newDeps {
		seen[d] = true
	}

	// Preserve local target deps from old list (e.g., ":mylib")
	// These are manual dependencies on same-package targets
	var kept []string
	for _, d := range oldDeps {
		if (strings.HasPrefix(d, ":") || isPreserved[d]) && !seen[d] {
			kept = append(kept, d)
			seen[d] = true
		}
	}

	// Return preserved deps first, then new deps
	return append(kept, newDeps...)
}

// withoutDeps returns deps without those in drop.
func withoutDeps(deps, drop []string) []string {
	if len(drop) == 0 {
		return deps
	}
	dropped := make(map[string]bool, len(drop))
	for _, d := range drop {
		dropped[d] = true
	}
	var result []string
	for _, d := range deps {
		if !dropped[d] {
			result = append(result, d)
		}
	}
	return result
}

// applyConditional sets a target's deps, held in its attr attribute (deps,
// npm_deps, ...), to what want returns in each configuration of space,
// preserving manual deps, and records the change. old gives the deps the
// target has in each configuration. Deps every configuration has are
// written as a plain list; the others as a select() (see
// conditional.SetLabels). It reports whether the target's deps changed.
func (r *SyncResult) applyConditional(target *starlark.Target, attr string, space conditions.Space,
	old func(conditions.Configuration) []string, want func(conditions.Configuration) (mapper.Want, error)) (bool, error) {
	preserved := target.GetPreservedLabels(attr)
	newDeps := make(map[string][]string, len(space.Configurations))
	changed := false
	var added, removed, kept, unmapped []string
	for _, config := range space.Configurations {
		oldDeps := old(config)
		w, err := want(config)
		if err != nil {
			return false, err
		}
		deps, k := mergeDeps(oldDeps, w, preserved)
		newDeps[config.String()] = deps
		if !stringSlicesEqual(oldDeps, deps) {
			changed = true
		}
		a, rm := diffDeps(oldDeps, deps)
		added = union(added, a)
		removed = union(removed, rm)
		if len(k) > 0 {
			kept = union(kept, k)
			unmapped = union(unmapped, w.Unmapped)
		}
	}

	if changed {
		// With markers, SetLabels writes only the auto-managed section.
		conditional.SetLabels(target, attr, space, func(config conditions.Configuration) []string {
			return withoutDeps(newDeps[config.String()], preserved)
		})
	}
	if changed || len(kept) > 0 {
		change := TargetChange{Target: target.Name, Added: added, Removed: removed}
		if len(kept) > 0 {
			change.Kept = kept
			change.Unmapped = unmapped
		}
		r.Changes = append(r.Changes, change)
	}
	return changed, nil
}

// applyOwned sets a target's attr, an attribute other than the deps that
// sync owns, to exactly what want returns in each configuration of space,
// written as a plain list or a select() like the deps. old gives the
// values the target has in each configuration. Values are compared as
// sets, and an absent attribute with no values stays absent. It reports
// whether the attribute changed.
func (r *SyncResult) applyOwned(target *starlark.Target, attr string, space conditions.Space,
	old func(conditions.Configuration) []string, want func(conditions.Configuration) ([]string, error)) (bool, error) {
	values := make(map[string][]string, len(space.Configurations))
	changed, anyValue := false, false
	var added, removed []string
	for _, config := range space.Configurations {
		oldValues := old(config)
		newValues, err := want(config)
		if err != nil {
			return false, err
		}
		if sameDepSet(oldValues, newValues) {
			newValues = oldValues
		}
		values[config.String()] = newValues
		anyValue = anyValue || len(newValues) > 0
		if !stringSlicesEqual(oldValues, newValues) {
			changed = true
		}
		a, rm := diffDeps(oldValues, newValues)
		added = union(added, a)
		removed = union(removed, rm)
	}
	if !changed || (target.GetAttribute(attr) == nil && !anyValue) {
		return false, nil
	}

	conditional.SetLabels(target, attr, space, func(config conditions.Configuration) []string { return values[config.String()] })
	r.Changes = append(r.Changes, TargetChange{Target: target.Name, Attribute: attr, Added: added, Removed: removed})
	return true, nil
}

// mergeDeps returns the deps a target with oldDeps gets in one
// configuration: w's labels, preserving manual ones. If w has unmapped
// imports its labels are incomplete, so no existing dep is removed:
// those that would have been are returned as kept. An existing dep in the
// Buck2 package of an unsynced dep is never removed either (nor reported
// as kept). Deps that are oldDeps in another order are oldDeps.
func mergeDeps(oldDeps []string, w mapper.Want, preserved []string) (newDeps, kept []string) {
	newDeps = mergeWithPreserved(oldDeps, preferVersioned(oldDeps, w.Labels), preserved)

	unsyncedPkgs := make(map[string]bool, len(w.Unsynced))
	for _, d := range w.Unsynced {
		unsyncedPkgs[labelPackage(d)] = true
	}
	newDeps, kept = keepExisting(oldDeps, newDeps, func(d string) bool {
		return len(w.Unmapped) > 0 || unsyncedPkgs[labelPackage(d)]
	})
	if len(w.Unmapped) == 0 {
		// Only unsynced deps were kept; they are reported on their own.
		kept = nil
	}
	if sameDepSet(oldDeps, newDeps) {
		// Sync manages which deps a target has, not their order.
		newDeps = oldDeps
	}
	return newDeps, kept
}

// union returns list with the strings of more it lacks appended, in order.
func union(list, more []string) []string {
	for _, s := range more {
		if !slices.Contains(list, s) {
			list = append(list, s)
		}
	}
	return list
}

// keepExisting returns the deps of oldDeps that newDeps lacks and keep
// accepts, as kept. If there are any, merged is oldDeps without the others,
// in their order, followed by the deps of newDeps that are not in oldDeps;
// otherwise merged is newDeps.
func keepExisting(oldDeps, newDeps []string, keep func(string) bool) (merged, kept []string) {
	inNew := make(map[string]bool, len(newDeps))
	for _, d := range newDeps {
		inNew[d] = true
	}
	inOld := make(map[string]bool, len(oldDeps))
	for _, d := range oldDeps {
		inOld[d] = true
		if !inNew[d] && keep(d) {
			kept = append(kept, d)
		}
	}
	if len(kept) == 0 {
		return newDeps, nil
	}

	for _, d := range oldDeps {
		if inNew[d] || keep(d) {
			merged = append(merged, d)
		}
	}
	for _, d := range newDeps {
		if !inOld[d] {
			merged = append(merged, d)
		}
	}
	return merged, kept
}

// sameDepSet reports whether a and b hold the same deps, in any order.
func sameDepSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	count := make(map[string]int, len(a))
	for _, d := range a {
		count[d]++
	}
	for _, d := range b {
		if count[d] == 0 {
			return false
		}
		count[d]--
	}
	return true
}

// preferVersioned returns mapped with each label replaced by the existing
// dep that pins a version of the same target, if there is one:
// "rustdeps//vendor/tokio@1.50.0:tokio" stands for "rustdeps//vendor/tokio:tokio".
func preferVersioned(oldDeps, mapped []string) []string {
	pinned := make(map[string]string)
	for _, d := range oldDeps {
		if u := unversioned(d); u != d {
			pinned[u] = d
		}
	}
	if len(pinned) == 0 {
		return mapped
	}
	result := make([]string, len(mapped))
	for i, d := range mapped {
		if p, ok := pinned[d]; ok {
			d = p
		}
		result[i] = d
	}
	return result
}

// unversioned strips an @version suffix from a label's package:
// "cell//vendor/foo@1.2.3:foo" -> "cell//vendor/foo:foo".
func unversioned(label string) string {
	pkg, name, found := strings.Cut(label, ":")
	slash := strings.LastIndex(pkg, "/")
	if at := strings.LastIndex(pkg, "@"); at > slash && at > 0 {
		pkg = pkg[:at]
	}
	if !found {
		return pkg
	}
	return pkg + ":" + name
}

// labelPackage returns a label's Buck2 package, without any @version:
// "//src/rust/composition:composition-full" -> "//src/rust/composition".
func labelPackage(label string) string {
	pkg, _, _ := strings.Cut(unversioned(label), ":")
	return pkg
}

// diffDeps returns added and removed deps.
func diffDeps(oldDeps, newDeps []string) (added, removed []string) {
	oldSet := make(map[string]bool)
	for _, d := range oldDeps {
		oldSet[d] = true
	}

	newSet := make(map[string]bool)
	for _, d := range newDeps {
		newSet[d] = true
	}

	for _, d := range newDeps {
		if !oldSet[d] {
			added = append(added, d)
		}
	}

	for _, d := range oldDeps {
		if !newSet[d] {
			removed = append(removed, d)
		}
	}

	return added, removed
}

// stringSlicesEqual compares two string slices.
func stringSlicesEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
