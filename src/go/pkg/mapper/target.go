package mapper

import (
	"fmt"
	"path/filepath"
	"slices"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// Package is one package being synced through its language: the
// configurations its deps are resolved for, and what each of its targets
// wants in them. It resolves each distinct request once, and collects what
// the resolutions report.
type Package struct {
	lang       Language
	dir        string
	dims       []string
	selfTarget string
	space      conditions.Space
	cache      map[string]PackageMapping

	// messages are the reports of every resolution, each once, in order.
	messages []string
	reported map[string]bool
}

// OpenPackage returns the package in dir, synced through lang in every
// configuration of space crossed with the on/off dimensions its deps
// depend on (e.g. Go build tags). projectRoot locates the package in the
// Buck2 project.
func OpenPackage(lang Language, projectRoot, dir string, space conditions.Space) (*Package, error) {
	dims, err := lang.Dimensions(dir)
	if err != nil {
		return nil, err
	}
	return &Package{
		lang:       lang,
		dir:        dir,
		dims:       dims,
		selfTarget: computeSelfTarget(dir, projectRoot),
		space:      space.WithDimensions(dims),
		cache:      make(map[string]PackageMapping),
		reported:   make(map[string]bool),
	}, nil
}

// Space returns the configurations the package's deps are resolved for.
func (p *Package) Space() conditions.Space { return p.space }

// Messages returns what the resolutions so far reported (imports that
// couldn't be mapped, deps sync doesn't own), each once, in order.
func (p *Package) Messages() []string { return p.messages }

// ResolveAll resolves the package in every configuration, as a library's
// deps, so that what can't be mapped is reported even if no target is
// synced.
func (p *Package) ResolveAll() error {
	for _, config := range p.space.Configurations {
		if _, err := p.resolve(config, Library, nil); err != nil {
			return err
		}
	}
	return nil
}

// Want is what a target wants in its deps in one configuration.
type Want struct {
	// Labels are the deps its sources or manifest need.
	Labels []string

	// Unmapped are the imports that couldn't be mapped: with any, Labels
	// is incomplete, so no existing dep should be removed.
	Unmapped []string

	// Unsynced are the targets of deps sync doesn't own: an existing dep
	// in the Buck2 package of one should never be removed.
	Unsynced []string

	// Canonical is the rule's Rule.Canonical.
	Canonical func(label string) string
}

// StandsFor returns the label an existing dep stands for: the label
// itself, unless the rule says otherwise (Rule.Canonical).
func (w Want) StandsFor(label string) string {
	if w.Canonical == nil {
		return label
	}
	return w.Canonical(label)
}

// Target is what sync wants for one target of a package.
type Target struct {
	pkg     *Package
	rule    Rule
	variant func(conditions.Configuration) map[string]starlark.AttributeValue

	// underTest is set for a test with a target_under_test.
	underTest bool
}

// Target returns what target, of rule, wants. It reports false, with the
// attribute, if one of its variant attributes can't be read.
func (p *Package) Target(target *starlark.Target, rule Rule) (*Target, string, bool) {
	variant, badAttr, ok := ReadVariant(target, rule.Variant, p.space)
	if !ok {
		return nil, badAttr, false
	}
	return &Target{
		pkg:       p,
		rule:      rule,
		variant:   variant,
		underTest: target.GetStringAttr("target_under_test") != "",
	}, "", true
}

// Deps returns what the target wants in its deps in config, where it has
// old.
func (t *Target) Deps(config conditions.Configuration, old []string) (Want, error) {
	m, err := t.pkg.resolve(config, t.rule.Kind, t.variant(config))
	if err != nil {
		return Want{}, err
	}
	// A test with a target_under_test or a same-package dep (":foo") gets
	// its library's deps through it.
	withLibrary := !t.underTest && !hasLocalDep(old)
	w := composeDeps(m, t.rule.Kind, withLibrary)
	w.Canonical = t.rule.Canonical
	return w, nil
}

// Owned returns the attributes other than the deps that the language sets
// for the target, in any configuration, sorted.
func (t *Target) Owned() ([]string, error) {
	var names []string
	for _, config := range t.pkg.space.Configurations {
		m, err := t.pkg.resolve(config, t.rule.Kind, t.variant(config))
		if err != nil {
			return nil, err
		}
		for name := range m.Attrs {
			if !slices.Contains(names, name) {
				names = append(names, name)
			}
		}
	}
	sort.Strings(names)
	return names, nil
}

// Attr returns the value the target wants for name, an attribute Owned
// returns, in config.
func (t *Target) Attr(config conditions.Configuration, name string) ([]string, error) {
	m, err := t.pkg.resolve(config, t.rule.Kind, t.variant(config))
	return m.Attrs[name], err
}

// composeDeps returns what a target of kind wants, from its package's
// mapping m in one configuration. A library or a binary wants exactly what
// its sources need. A test wants its test-only deps and, withLibrary, its
// package's library deps too; test-only imports that couldn't be mapped
// leave only a test's deps incomplete.
func composeDeps(m PackageMapping, kind TargetKind, withLibrary bool) Want {
	w := Want{Unsynced: unsyncedTargets(m)}
	switch kind {
	case Library, Binary:
		w.Labels = DepsToTargets(m.Deps)
		w.Unmapped = m.UnmappedImports
	case Test:
		seen := make(map[string]bool)
		add := func(deps []MappedDep) {
			for _, d := range DepsToTargets(deps) {
				if !seen[d] {
					seen[d] = true
					w.Labels = append(w.Labels, d)
				}
			}
		}
		if withLibrary {
			add(m.Deps)
		}
		add(m.TestDeps)
		w.Unmapped = append(append([]string(nil), m.UnmappedImports...), m.UnmappedTestImports...)
	}
	return w
}

// resolve returns the package's deps in config, for a target of kind and
// its variant.
// Deps on the package's own target are dropped (e.g. when syncing
// src/python/cargo, //src/python/cargo:cargo).
func (p *Package) resolve(config conditions.Configuration, kind TargetKind, variant map[string]starlark.AttributeValue) (PackageMapping, error) {
	req := Request{Config: config.Project(p.dims), Kind: kind, Variant: variant}
	key := fmt.Sprintf("%s|%d|%s", req.Config, kind, variantKey(variant))
	if m, ok := p.cache[key]; ok {
		return m, nil
	}
	m, err := p.lang.ResolveDeps(p.dir, req)
	if err != nil {
		return m, err
	}
	m.Deps = filterSelfReference(m.Deps, p.selfTarget)
	m.TestDeps = filterSelfReference(m.TestDeps, p.selfTarget)
	p.cache[key] = m

	for _, unmapped := range m.UnmappedImports {
		p.report(fmt.Sprintf("unmapped import: %s", unmapped))
	}
	for _, unmapped := range m.UnmappedTestImports {
		p.report(fmt.Sprintf("unmapped test import: %s", unmapped))
	}
	for _, u := range m.UnsyncedDeps {
		p.report(fmt.Sprintf("%s dependency %s not synced", u.Reason, u.Dep.ImportPath))
	}
	return m, nil
}

// report records a message unless it already was.
func (p *Package) report(msg string) {
	if !p.reported[msg] {
		p.reported[msg] = true
		p.messages = append(p.messages, msg)
	}
}

// variantKey identifies a variant.
func variantKey(variant map[string]starlark.AttributeValue) string {
	names := make([]string, 0, len(variant))
	for name := range variant {
		names = append(names, name)
	}
	sort.Strings(names)
	var b strings.Builder
	for _, name := range names {
		fmt.Fprintf(&b, "%s=%s;", name, starlark.Render(variant[name]))
	}
	return b.String()
}

// unsyncedTargets returns the targets of a mapping's unsynced deps.
func unsyncedTargets(m PackageMapping) []string {
	var targets []string
	for _, u := range m.UnsyncedDeps {
		if u.Dep.Target != "" {
			targets = append(targets, u.Dep.Target)
		}
	}
	return targets
}

// hasLocalDep returns true if deps contains a local target dep (":foo").
func hasLocalDep(deps []string) bool {
	for _, d := range deps {
		if strings.HasPrefix(d, ":") {
			return true
		}
	}
	return false
}

// computeSelfTarget computes the Buck target for the current package.
// e.g., "/path/to/src/python/cargo" with projectRoot "/path/to" -> "//src/python/cargo:cargo"
func computeSelfTarget(pkgDir, projectRoot string) string {
	relPath, err := filepath.Rel(projectRoot, pkgDir)
	if err != nil {
		return ""
	}
	// relPath is like "src/python/cargo"
	targetName := filepath.Base(relPath)
	return fmt.Sprintf("//%s:%s", relPath, targetName)
}

// filterSelfReference removes deps that match the selfTarget.
func filterSelfReference(deps []MappedDep, selfTarget string) []MappedDep {
	if selfTarget == "" {
		return deps
	}
	var filtered []MappedDep
	for _, dep := range deps {
		if dep.Target != selfTarget {
			filtered = append(filtered, dep)
		}
	}
	return filtered
}
