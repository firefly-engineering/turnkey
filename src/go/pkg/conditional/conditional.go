// Package conditional reads and writes a conditional attribute: a target
// attribute whose value depends on the build configuration, written as a
// plain value or as [<labels>] + select({<key>: <value>, ...}) over a
// conditions.Space.
//
// It is the one place that decides what such a value is in each
// configuration, and how a value per configuration is written, so rules
// sync, the mapper plug-ins and the cell generators agree on both.
package conditional

import (
	"fmt"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// Read reads target's attribute attr as its value in each configuration of
// space:
//
//   - An absent attribute is nil in every configuration.
//   - A value that isn't a select() is itself in every configuration.
//   - A select()'s value is its plain part followed by the value of the
//     branch that applies (conditions.Matcher.Branch: the most specific
//     key matching, else DEFAULT). When both are label lists, it is a
//     StringListValue holding each label once; a select() alone whose
//     branch is another kind of value (True, a string) gives that value.
//   - A configuration no branch applies to, in which the build would fail,
//     gets the plain part alone: its labels, or nil for a select() alone.
//
// It returns an error if the select() can't be read: a key the space
// doesn't know (neither DEFAULT nor one sync writes), or a plain part
// followed by a branch that isn't a list of labels.
func Read(target *starlark.Target, attr string, space conditions.Space) (func(conditions.Configuration) starlark.AttributeValue, error) {
	a := target.GetAttribute(attr)
	if a == nil {
		return func(conditions.Configuration) starlark.AttributeValue { return nil }, nil
	}
	sel, ok := a.Value.(starlark.SelectValue)
	if !ok {
		value := a.Value
		return func(conditions.Configuration) starlark.AttributeValue { return value }, nil
	}
	common, ok := starlark.Labels(sel.Common)
	if !ok {
		return nil, fmt.Errorf("%s: the part before select() is not a list of labels", attr)
	}
	keys := make([]string, len(sel.Branches))
	for i, b := range sel.Branches {
		keys[i] = b.Key
		if _, ok := starlark.Labels(b.Value); sel.Common != nil && (!ok || b.Value == nil) {
			return nil, fmt.Errorf("%s: select() branch %q is not a list of labels", attr, b.Key)
		}
	}
	matcher, err := space.Matcher(keys)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", attr, err)
	}
	return func(config conditions.Configuration) starlark.AttributeValue {
		var branch starlark.AttributeValue
		if i := matcher.Branch(config); i >= 0 {
			branch = sel.Branches[i].Value
		}
		extra, ok := starlark.Labels(branch)
		if !ok {
			return branch
		}
		if sel.Common == nil && branch == nil {
			return nil
		}
		labels := append(append([]string(nil), common...), extra...)
		return starlark.StringListValue{Values: dedupe(labels)}
	}, nil
}

// ReadLabels reads target's label-list attribute attr (deps, npm_deps, ...)
// as the labels it has in each configuration of space, as Read does. An
// absent attribute has no labels. It returns an error if the attribute
// isn't a list of labels, or a select() Read can read whose branches all
// are: a computed value (a variable, a concatenation, a conditional) isn't
// read, since whoever writes back would replace the expression with values
// of its own.
func ReadLabels(target *starlark.Target, attr string, space conditions.Space) (func(conditions.Configuration) []string, error) {
	if a := target.GetAttribute(attr); a != nil && !isLabels(a.Value) {
		return nil, fmt.Errorf("%s: %s is not a list of labels", attr, a.Value)
	}
	read, err := Read(target, attr, space)
	if err != nil {
		return nil, err
	}
	return func(config conditions.Configuration) []string {
		labels, _ := starlark.Labels(read(config))
		return labels
	}, nil
}

// isLabels reports whether a value is a list of labels in every
// configuration: a list, or a select() of lists.
func isLabels(value starlark.AttributeValue) bool {
	sel, ok := value.(starlark.SelectValue)
	if !ok {
		_, ok := starlark.Labels(value)
		return ok
	}
	if _, ok := starlark.Labels(sel.Common); !ok {
		return false
	}
	for _, b := range sel.Branches {
		if _, ok := starlark.Labels(b.Value); !ok || b.Value == nil {
			return false
		}
	}
	return true
}

// LabelsValue returns the value of a label-list attribute that has
// labels(config) in each configuration of space: the labels every
// configuration has, as a plain list, followed by a select() of the others
// when they differ, keyed as conditions.Space.Split keys them. It returns
// nil when no configuration has a label.
func LabelsValue(space conditions.Space, labels func(conditions.Configuration) []string) starlark.AttributeValue {
	split := space.Split(labels)
	if !split.IsConditional() {
		if len(split.Common) == 0 {
			return nil
		}
		return starlark.StringListValue{Values: split.Common}
	}
	sel := starlark.SelectValue{Branches: branches(split)}
	if len(split.Common) > 0 {
		sel.Common = starlark.StringListValue{Values: split.Common}
	}
	return sel
}

// SetLabels sets target's label-list attribute attr to have labels(config)
// in each configuration of space, written as LabelsValue writes it, or as
// [] when no configuration has a label. The markers of the attribute's
// plain part are kept: the plain part becomes its auto-managed section
// (see starlark.Target.SetSelect).
func SetLabels(target *starlark.Target, attr string, space conditions.Space, labels func(conditions.Configuration) []string) {
	split := space.Split(labels)
	target.SetSelect(attr, split.Common, branches(split))
}

// branches returns a split's select() entries as written.
func branches(split conditions.Split) []starlark.SelectBranch {
	var result []starlark.SelectBranch
	for _, b := range split.Branches {
		result = append(result, starlark.SelectBranch{Key: b.Key, Value: starlark.StringListValue{Values: b.Labels}})
	}
	return result
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
