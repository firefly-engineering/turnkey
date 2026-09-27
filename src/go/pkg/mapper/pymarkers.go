package mapper

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/pep508"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
	"github.com/pelletier/go-toml/v2"
)

// pyMember is a uv workspace member: where it is, and the dependencies its
// pyproject.toml declares.
type pyMember struct {
	// dir is the member's directory, relative to the project root.
	dir string

	// name is its distribution name, normalized.
	name string

	// requires are its [project] dependencies, by normalized name.
	requires map[string]pep508.Requirement

	// extras are its [project.optional-dependencies], by normalized extra.
	extras map[string][]pep508.Requirement
}

// loadPyMembers reads the uv workspace members' pyproject.toml.
func loadPyMembers(projectRoot string) ([]*pyMember, error) {
	var root struct {
		Tool struct {
			UV struct {
				Workspace struct {
					Members []string `toml:"members"`
				} `toml:"workspace"`
			} `toml:"uv"`
		} `toml:"tool"`
	}
	if err := readTOML(filepath.Join(projectRoot, "pyproject.toml"), &root); err != nil {
		return nil, err
	}
	var members []*pyMember
	for _, pattern := range root.Tool.UV.Workspace.Members {
		dirs, err := filepath.Glob(filepath.Join(projectRoot, pattern))
		if err != nil {
			return nil, fmt.Errorf("workspace member %q: %w", pattern, err)
		}
		for _, dir := range dirs {
			member, err := loadPyMember(projectRoot, dir)
			if err != nil {
				return nil, err
			}
			if member != nil {
				members = append(members, member)
			}
		}
	}
	return members, nil
}

// loadPyMember reads one member's pyproject.toml, or returns nil if the
// directory has none.
func loadPyMember(projectRoot, dir string) (*pyMember, error) {
	var pyproject struct {
		Project struct {
			Name                 string              `toml:"name"`
			Dependencies         []string            `toml:"dependencies"`
			OptionalDependencies map[string][]string `toml:"optional-dependencies"`
		} `toml:"project"`
	}
	path := filepath.Join(dir, "pyproject.toml")
	if _, err := os.Stat(path); err != nil {
		return nil, nil
	}
	if err := readTOML(path, &pyproject); err != nil {
		return nil, err
	}
	rel, err := filepath.Rel(projectRoot, dir)
	if err != nil {
		return nil, err
	}
	member := &pyMember{
		dir:      filepath.ToSlash(rel),
		name:     pep508.NormalizeName(pyproject.Project.Name),
		requires: make(map[string]pep508.Requirement),
		extras:   make(map[string][]pep508.Requirement),
	}
	for _, spec := range pyproject.Project.Dependencies {
		req, err := pep508.ParseRequirement(spec)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", path, err)
		}
		member.requires[req.Name] = req
	}
	for extra, specs := range pyproject.Project.OptionalDependencies {
		extra = pep508.NormalizeName(extra)
		for _, spec := range specs {
			req, err := pep508.ParseRequirement(spec)
			if err != nil {
				return nil, fmt.Errorf("%s: %w", path, err)
			}
			member.extras[extra] = append(member.extras[extra], req)
		}
	}
	return member, nil
}

func readTOML(path string, v any) error {
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if err := toml.Unmarshal(content, v); err != nil {
		return fmt.Errorf("parsing %s: %w", path, err)
	}
	return nil
}

// memberOf returns the member whose directory holds pkgDir (relative to
// the project root), the innermost one, or nil.
func (l *pythonLanguage) memberOf(rel string) *pyMember {
	var best *pyMember
	for _, m := range l.members {
		if (rel == m.dir || strings.HasPrefix(rel, m.dir+"/")) && (best == nil || len(m.dir) > len(best.dir)) {
			best = m
		}
	}
	return best
}

// memberNamed returns the member whose distribution is name, or nil.
func (l *pythonLanguage) memberNamed(name string) *pyMember {
	for _, m := range l.members {
		if m.name == name {
			return m
		}
	}
	return nil
}

// distKey returns the distribution a dep's target builds, normalized: a
// member's name, or a pydeps package's (whose key uses _ for -).
func (l *pythonLanguage) distKey(target string) string {
	for _, m := range l.members {
		if target == memberTarget(m) {
			return m.name
		}
	}
	if pkg, ok := strings.CutPrefix(target, l.cfg.ExternalCell+"//vendor/"); ok {
		key, _, _ := strings.Cut(pkg, ":")
		return pep508.NormalizeName(key)
	}
	return ""
}

func memberTarget(m *pyMember) string {
	return fmt.Sprintf("//%s:%s", m.dir, filepath.Base(m.dir))
}

// requirementTarget maps a requirement to its target: a member's, or the
// pydeps cell's for a vendored package.
func (l *pythonLanguage) requirementTarget(req pep508.Requirement) (MappedDep, bool) {
	if m := l.memberNamed(req.Name); m != nil {
		return MappedDep{Target: memberTarget(m), Type: DependencyInternal, ImportPath: req.Name}, true
	}
	key := strings.ReplaceAll(req.Name, "-", "_")
	if !l.cfg.ExternalDeps[key] {
		return MappedDep{}, false
	}
	return MappedDep{
		Target:     fmt.Sprintf("%s//vendor/%s:%s", l.cfg.ExternalCell, key, key),
		Type:       DependencyExternal,
		ImportPath: req.Name,
	}, true
}

// pythonVersion returns the Python toolchain's full version, from the
// python3 on PATH (the toolchain's, in a turnkey shell), or "" if there is
// none.
func (l *pythonLanguage) pythonVersion() string {
	if l.version == nil {
		version := ""
		out, err := exec.Command("python3", "-c", "import platform; print(platform.python_version())").Output()
		if err == nil {
			version = strings.TrimSpace(string(out))
		}
		l.version = &version
	}
	return *l.version
}

// markerHolds reports whether a requirement's marker holds in config (a
// platform, or none) for the Python toolchain, with extra. A marker whose
// variables aren't known (a platform one without a platform, a Python one
// without python3) is taken to hold: sync can't tell, so it keeps the dep.
func (l *pythonLanguage) markerHolds(marker *pep508.Marker, config conditions.Configuration, extra string) bool {
	if marker == nil {
		return true
	}
	version := l.pythonVersion()
	env, hasPlatform := pep508.PlatformEnv(config[conditions.OS], config[conditions.CPU], version)
	for _, v := range marker.Variables() {
		if !hasPlatform && slices.Contains(pep508.PlatformVariables, v) {
			return true
		}
		if version == "" && strings.Contains(v, "version") {
			return true
		}
	}
	if !hasPlatform {
		env = pep508.Env{"python_version": majorMinorVersion(version), "python_full_version": version,
			"implementation_name": "cpython", "platform_python_implementation": "CPython"}
	}
	env["extra"] = extra
	return marker.Evaluate(env)
}

func majorMinorVersion(v string) string {
	parts := strings.SplitN(v, ".", 3)
	if len(parts) < 2 {
		return v
	}
	return parts[0] + "." + parts[1]
}

// targetExtras returns the extras a target's variant builds with,
// normalized.
func targetExtras(variant map[string]starlark.AttributeValue) []string {
	extras, _ := starlark.Labels(variant["extras"])
	var normalized []string
	for _, e := range extras {
		normalized = append(normalized, pep508.NormalizeName(e))
	}
	sort.Strings(normalized)
	return normalized
}

// applyMarkers keeps the deps of a member's package that its pyproject.toml
// declares where their markers hold, and adds the dependencies of the
// target's extras:
//   - a dependency in [project] dependencies is kept where its marker holds
//   - one only in [project.optional-dependencies] is kept if one of the
//     target's extras declares it, where that marker holds
//   - one the member doesn't declare (an import sync maps on its own) is
//     kept
//
// Each extra's dependencies are added where their markers hold, whether
// the sources import them or not.
func (l *pythonLanguage) applyMarkers(m PackageMapping, member *pyMember, req Request) PackageMapping {
	extras := targetExtras(req.Variant)
	keep := func(deps []MappedDep) []MappedDep {
		var kept []MappedDep
		for _, dep := range deps {
			name := l.distKey(dep.Target)
			if r, ok := member.requires[name]; ok {
				if l.markerHolds(r.Marker, req.Config, "") {
					kept = append(kept, dep)
				}
				continue
			}
			declared, enabled := false, false
			for extra, reqs := range member.extras {
				for _, r := range reqs {
					if r.Name != name {
						continue
					}
					declared = true
					if slices.Contains(extras, extra) && l.markerHolds(r.Marker, req.Config, extra) {
						enabled = true
					}
				}
			}
			if !declared || enabled {
				kept = append(kept, dep)
			}
		}
		return kept
	}
	m.Deps = keep(m.Deps)
	m.TestDeps = keep(m.TestDeps)

	for _, extra := range extras {
		for _, r := range member.extras[extra] {
			if !l.markerHolds(r.Marker, req.Config, extra) {
				continue
			}
			dep, ok := l.requirementTarget(r)
			if !ok {
				m.UnmappedImports = append(m.UnmappedImports, fmt.Sprintf("%s (extra %s)", r.Name, extra))
				continue
			}
			if dep.Target == memberTarget(member) || slices.ContainsFunc(m.Deps, func(d MappedDep) bool { return d.Target == dep.Target }) {
				continue
			}
			m.Deps = append(m.Deps, dep)
		}
	}
	sortDeps(m.Deps)
	return m
}
