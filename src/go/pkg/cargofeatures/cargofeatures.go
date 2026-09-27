// Package cargofeatures expands a request for a crate's features the way
// Cargo does: what a set of requested features turns on in the crate.
//
// It mirrors turnkey.cargo.features.activate (src/python/cargo), which the
// rustdeps cell uses for vendored crates; testdata/activation-vectors.json
// holds test cases both run.
//
// Reference: https://doc.rust-lang.org/cargo/reference/features.html
package cargofeatures

import (
	"slices"
	"sort"
	"strings"
)

// Crate is what feature activation reads from a crate's manifest.
type Crate struct {
	// Features is the [features] table: each feature's items.
	Features map[string][]string

	// Optional and Required are the manifest keys of its dependencies, in
	// any dependency table: a key optional in one table and required in
	// another is in both.
	Optional, Required map[string]bool
}

// Activation is what a set of requested features turns on in a crate.
type Activation struct {
	// Features are the enabled features, transitively expanded: "default"
	// is expanded but not reported, and dep: or forwarding items aren't
	// features.
	Features []string

	// OptionalDeps are the optional dependencies (by manifest key) the
	// features activate.
	OptionalDeps []string

	// DepFeatures are the features to request on dependencies (by manifest
	// key): from "dep/feature" items, and from "dep?/feature" items whose
	// dependency is active.
	DepFeatures map[string][]string
}

// Activate expands requested features of crate as Cargo does. Features in
// remove are treated as absent, so what only they would turn on stays off;
// removing "default" drops the crate's default set.
func Activate(crate Crate, requested, remove []string) Activation {
	removed := make(map[string]bool, len(remove))
	for _, f := range remove {
		removed[f] = true
	}
	// An optional dependency named with "dep:" anywhere has no implicit
	// feature of its own name.
	namedWithDep := make(map[string]bool)
	for _, items := range crate.Features {
		for _, item := range items {
			if dep, ok := strings.CutPrefix(item, "dep:"); ok {
				namedWithDep[dep] = true
			}
		}
	}

	type forward struct {
		dep, feature string
		weak         bool
	}
	enabled := make(map[string]bool)
	optionalDeps := make(map[string]bool)
	var forwards []forward
	toProcess := append([]string(nil), requested...)
	for len(toProcess) > 0 {
		feature := toProcess[len(toProcess)-1]
		toProcess = toProcess[:len(toProcess)-1]
		if removed[feature] || enabled[feature] {
			continue
		}
		if dep, ok := strings.CutPrefix(feature, "dep:"); ok {
			optionalDeps[dep] = true
			continue
		}
		if depPart, depFeature, ok := strings.Cut(feature, "/"); ok {
			dep, weak := strings.CutSuffix(depPart, "?")
			forwards = append(forwards, forward{dep, depFeature, weak})
			if !weak && crate.Optional[dep] {
				optionalDeps[dep] = true
				// ...and turns on the feature of the dependency's name, if
				// there is one (explicit, or implicit)
				if _, defined := crate.Features[dep]; defined || !namedWithDep[dep] {
					toProcess = append(toProcess, dep)
				}
			}
			continue
		}
		enabled[feature] = true
		if items, defined := crate.Features[feature]; defined {
			toProcess = append(toProcess, items...)
		} else if crate.Optional[feature] && !namedWithDep[feature] {
			optionalDeps[feature] = true
		}
	}

	result := Activation{DepFeatures: make(map[string][]string)}
	for _, f := range forwards {
		if f.weak && !crate.Required[f.dep] && !optionalDeps[f.dep] {
			continue
		}
		if !slices.Contains(result.DepFeatures[f.dep], f.feature) {
			result.DepFeatures[f.dep] = append(result.DepFeatures[f.dep], f.feature)
		}
	}
	for _, features := range result.DepFeatures {
		sort.Strings(features)
	}
	delete(enabled, "default")
	result.Features = sortedKeys(enabled)
	result.OptionalDeps = sortedKeys(optionalDeps)
	return result
}

func sortedKeys(set map[string]bool) []string {
	keys := make([]string, 0, len(set))
	for k := range set {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
