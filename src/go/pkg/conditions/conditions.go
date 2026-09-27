// Package conditions models the build configurations a target's deps can
// depend on, and the select() rules sync writes for deps that differ
// between them.
//
// A configuration is one value for each of a set of named dimensions, and
// each value is backed by a Buck2 constraint:
//
//   - os: the platform's operating system (config//os:linux, ...)
//   - cpu: the platform's CPU (config//cpu:x86_64, ...)
//   - on/off dimensions a language adds for a package (OnOff), e.g. one
//     per Go build tag that matters, each backed by a constraint's [set]
//     and [unset] values
//
// The platforms turnkey builds for (the buck2.platforms Nix option, passed
// through .turnkey/sync.toml) fix which os and cpu values go together. Sync
// evaluates every configuration, so what it writes never depends on the
// host it runs on.
//
// turnkey.cfg (src/python/cfg) and nix/buck2/platforms.nix mirror Split
// for the os and cpu dimensions, for the cell generators;
// testdata/split-vectors.json holds test cases all three run.
package conditions

import (
	"fmt"
	"slices"
	"sort"
	"strings"
)

// The dimensions every platform has.
const (
	OS  = "os"
	CPU = "cpu"
)

// The values of an on/off dimension.
const (
	Set   = "set"
	Unset = "unset"
)

// DefaultSettingsPackage is the Buck2 package holding turnkey's combined
// config_settings (one per platform, named "<os>-<cpu>"), when sync.toml
// doesn't name one.
const DefaultSettingsPackage = "toolchains//conditions"

// Platform is one platform turnkey builds for, in Buck2's constraint names.
type Platform struct {
	// OS is the operating system, a value of config//os (e.g. "linux").
	OS string `toml:"os"`

	// CPU is the CPU, a value of config//cpu (e.g. "x86_64").
	CPU string `toml:"cpu"`
}

// String returns the platform as "<os>-<cpu>", the name of its combined
// config_setting.
func (p Platform) String() string { return p.OS + "-" + p.CPU }

// Configuration assigns a value to each of a set of dimensions.
type Configuration map[string]string

// String returns the configuration as "dim=value,..." in dimension name
// order, e.g. "cpu=arm64,os=linux". It identifies the configuration.
func (c Configuration) String() string {
	names := make([]string, 0, len(c))
	for name := range c {
		names = append(names, name)
	}
	sort.Strings(names)
	parts := make([]string, len(names))
	for i, name := range names {
		parts[i] = name + "=" + c[name]
	}
	return strings.Join(parts, ",")
}

// Project returns the configuration restricted to dims.
func (c Configuration) Project(dims []string) Configuration {
	projected := make(Configuration, len(dims))
	for _, dim := range dims {
		if value, ok := c[dim]; ok {
			projected[dim] = value
		}
	}
	return projected
}

// Includes reports whether c assigns every dimension of partial the same
// value.
func (c Configuration) Includes(partial Configuration) bool {
	for dim, value := range partial {
		if c[dim] != value {
			return false
		}
	}
	return true
}

// Dimension is a named axis of the configuration.
type Dimension struct {
	// Name identifies the dimension, e.g. "os".
	Name string

	// Values are the dimension's values, in the order keys are written.
	Values []string

	// key returns the select() key matching one value, alone.
	key func(value string) string

	// token names a value in a combined config_setting's name.
	token func(value string) string
}

// Key returns the select() key that matches value alone, e.g.
// "config//os:linux".
func (d Dimension) Key(value string) string { return d.key(value) }

// Space is the set of configurations sync evaluates: one per platform.
type Space struct {
	// Dimensions are the space's dimensions, in the order sync prefers
	// them for select() keys.
	Dimensions []Dimension

	// Configurations are the space's configurations, one per platform.
	Configurations []Configuration

	// settings is the Buck2 package of the combined config_settings.
	settings string
}

// NewSpace returns the space of the given platforms. settings is the Buck2
// package holding one combined config_setting per platform
// (DefaultSettingsPackage when empty). With no platforms, the space has a
// single configuration with no dimensions: deps can't depend on it.
func NewSpace(platforms []Platform, settings string) Space {
	if settings == "" {
		settings = DefaultSettingsPackage
	}
	space := Space{settings: settings}
	if len(platforms) == 0 {
		space.Configurations = []Configuration{{}}
		return space
	}

	var oses, cpus []string
	seen := make(map[string]bool)
	for _, p := range platforms {
		config := Configuration{OS: p.OS, CPU: p.CPU}
		if seen[config.String()] {
			continue
		}
		seen[config.String()] = true
		space.Configurations = append(space.Configurations, config)
		oses = appendNew(oses, p.OS)
		cpus = appendNew(cpus, p.CPU)
	}
	sort.Strings(oses)
	sort.Strings(cpus)
	same := func(v string) string { return v }
	space.Dimensions = []Dimension{
		{Name: OS, Values: oses, key: func(v string) string { return "config//os:" + v }, token: same},
		{Name: CPU, Values: cpus, key: func(v string) string { return "config//cpu:" + v }, token: same},
	}
	return space
}

// OnOff is an on/off dimension a language adds to the space a package's
// deps are resolved in, e.g. a Go build tag's. Its values are Set and
// Unset.
type OnOff struct {
	// Name identifies the dimension, e.g. "go_tag:integration".
	Name string `json:"name"`

	// Constraint is the Buck2 constraint whose [set] and [unset] values
	// back the dimension's, e.g. "prelude//go/tags/constraints:integration".
	Constraint string `json:"constraint"`

	// Token names Set in a combined config_setting's name, e.g.
	// "integration"; Unset is "no_<token>".
	Token string `json:"token"`
}

// WithDimensions returns the space with the on/off dimensions dims added,
// sorted by name, each crossing every configuration with both its values.
// Names already in the space are ignored.
func (s Space) WithDimensions(dims []OnOff) Space {
	var added []OnOff
	for _, d := range dims {
		_, have := s.dimension(d.Name)
		if !have && !slices.ContainsFunc(added, func(a OnOff) bool { return a.Name == d.Name }) {
			added = append(added, d)
		}
	}
	if len(added) == 0 {
		return s
	}
	sort.Slice(added, func(i, j int) bool { return added[i].Name < added[j].Name })

	extended := Space{settings: s.settings, Dimensions: append([]Dimension(nil), s.Dimensions...)}
	configs := s.Configurations
	for _, d := range added {
		extended.Dimensions = append(extended.Dimensions, Dimension{
			Name:   d.Name,
			Values: []string{Set, Unset},
			key: func(v string) string {
				return fmt.Sprintf("%s[%s]", d.Constraint, v)
			},
			token: func(v string) string {
				if v == Set {
					return d.Token
				}
				return "no_" + d.Token
			},
		})
		var crossed []Configuration
		for _, config := range configs {
			for _, v := range []string{Set, Unset} {
				c := make(Configuration, len(config)+1)
				for k, val := range config {
					c[k] = val
				}
				c[d.Name] = v
				crossed = append(crossed, c)
			}
		}
		configs = crossed
	}
	extended.Configurations = configs
	return extended
}

// appendNew appends s to list unless it is already there.
func appendNew(list []string, s string) []string {
	if slices.Contains(list, s) {
		return list
	}
	return append(list, s)
}

// dimension returns the space's dimension named name.
func (s Space) dimension(name string) (Dimension, bool) {
	for _, d := range s.Dimensions {
		if d.Name == name {
			return d, true
		}
	}
	return Dimension{}, false
}

// key returns the select() key matching the configurations that assign
// partial's values: the dimension's own key for a single dimension, or the
// combined config_setting for several.
func (s Space) key(dims []string, partial Configuration) string {
	if len(dims) == 1 {
		d, _ := s.dimension(dims[0])
		return d.Key(partial[dims[0]])
	}
	tokens := make([]string, len(dims))
	for i, dim := range dims {
		d, _ := s.dimension(dim)
		tokens[i] = d.token(partial[dim])
	}
	return s.settings + ":" + strings.Join(tokens, "-")
}

// keys returns every select() key the space can write, with the partial
// configuration each one matches.
func (s Space) keys() map[string]Configuration {
	keys := make(map[string]Configuration)
	for _, dims := range s.dimensionSets() {
		for _, config := range s.Configurations {
			partial := config.Project(dims)
			keys[s.key(dims, partial)] = partial
		}
	}
	return keys
}

// dimensionSets returns every non-empty set of the space's dimensions, by
// increasing size, each in the order of Dimensions. The first one that
// explains a difference between configurations gives the smallest keys.
func (s Space) dimensionSets() [][]string {
	var sets [][]string
	n := len(s.Dimensions)
	for size := 1; size <= n; size++ {
		var pick func(start int, set []string)
		pick = func(start int, set []string) {
			if len(set) == size {
				sets = append(sets, append([]string(nil), set...))
				return
			}
			for i := start; i < n; i++ {
				pick(i+1, append(set, s.Dimensions[i].Name))
			}
		}
		pick(0, nil)
	}
	return sets
}

// Branch is one select() entry: the labels a key adds.
type Branch struct {
	// Key is the select() key, e.g. "config//os:linux".
	Key string

	// Labels are the labels it adds to the common ones.
	Labels []string
}

// Split is a label list over a space: the labels every configuration has,
// and per select() key the labels only some have. It is written as
// [<Common>] + select({<key>: [<labels>], ...}), with no DEFAULT branch, or
// as a plain list when there are no branches.
type Split struct {
	// Common are the labels of every configuration.
	Common []string

	// Branches are the select() entries, sorted by key; nil when every
	// configuration has the same labels.
	Branches []Branch
}

// IsConditional reports whether the split needs a select().
func (sp Split) IsConditional() bool { return len(sp.Branches) > 0 }

// Split splits each configuration's labels into common and conditional
// ones. Common labels keep the order of the first configuration; a
// branch's labels keep the order of its first configuration. Keys are the
// smallest exact ones: the fewest dimensions (preferring the space's order)
// whose values determine each configuration's extra labels.
func (s Space) Split(labels func(Configuration) []string) Split {
	per := make([][]string, len(s.Configurations))
	for i, config := range s.Configurations {
		per[i] = dedupe(labels(config))
	}

	var common []string
	for _, label := range per[0] {
		inAll := true
		for _, other := range per[1:] {
			if !slices.Contains(other, label) {
				inAll = false
				break
			}
		}
		if inAll {
			common = append(common, label)
		}
	}

	extra := make([][]string, len(per))
	anyExtra := false
	for i, list := range per {
		for _, label := range list {
			if !slices.Contains(common, label) {
				extra[i] = append(extra[i], label)
			}
		}
		anyExtra = anyExtra || len(extra[i]) > 0
	}
	if !anyExtra {
		return Split{Common: common}
	}

	for _, dims := range s.dimensionSets() {
		branches, ok := s.branches(dims, extra)
		if ok {
			return Split{Common: common, Branches: branches}
		}
	}
	// Every configuration is distinct in the full set of dimensions, so
	// the last set always explains the differences.
	panic("conditions: no dimension set explains the configurations' differences")
}

// branches keys each configuration's extra labels by its values of dims. It
// reports false if two configurations with the same values have different
// extras: dims don't explain the difference.
func (s Space) branches(dims []string, extra [][]string) ([]Branch, bool) {
	byKey := make(map[string]int)
	var branches []Branch
	for i, config := range s.Configurations {
		key := s.key(dims, config.Project(dims))
		if j, ok := byKey[key]; ok {
			if !sameSet(branches[j].Labels, extra[i]) {
				return nil, false
			}
			continue
		}
		byKey[key] = len(branches)
		branches = append(branches, Branch{Key: key, Labels: extra[i]})
	}
	sort.Slice(branches, func(i, j int) bool { return branches[i].Key < branches[j].Key })
	return branches, true
}

// DefaultKey is the select() key that matches when no other does. Sync
// reads it but never writes it.
const DefaultKey = "DEFAULT"

// Matcher tells which of a select()'s keys matches each configuration of
// a space.
type Matcher struct {
	// matches holds, per key, the partial configuration it matches; nil
	// for DEFAULT.
	matches []Configuration
}

// Matcher returns the matcher of a select() with keys, or an error if a
// key is neither DEFAULT nor one the space writes: sync can't tell which
// configurations it matches.
func (s Space) Matcher(keys []string) (*Matcher, error) {
	known := s.keys()
	m := &Matcher{}
	for _, key := range keys {
		if key == DefaultKey {
			m.matches = append(m.matches, nil)
			continue
		}
		partial, ok := known[key]
		if !ok {
			return nil, fmt.Errorf("select() key %q is not one sync knows", key)
		}
		m.matches = append(m.matches, partial)
	}
	return m, nil
}

// Branch returns the index of the key that applies in config: the most
// specific one matching it, or DEFAULT if none does. It returns -1 if no
// key applies (the build would fail in config).
func (m *Matcher) Branch(config Configuration) int {
	best, bestSize, defaultBranch := -1, -1, -1
	for i, partial := range m.matches {
		if partial == nil {
			defaultBranch = i
			continue
		}
		if config.Includes(partial) && len(partial) > bestSize {
			best, bestSize = i, len(partial)
		}
	}
	if best < 0 {
		return defaultBranch
	}
	return best
}

// dedupe returns list without repeated labels, in order.
func dedupe(list []string) []string {
	seen := make(map[string]bool, len(list))
	var result []string
	for _, s := range list {
		if !seen[s] {
			seen[s] = true
			result = append(result, s)
		}
	}
	return result
}

// sameSet reports whether a and b hold the same labels, in any order.
func sameSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for _, s := range a {
		if !slices.Contains(b, s) {
			return false
		}
	}
	return true
}
