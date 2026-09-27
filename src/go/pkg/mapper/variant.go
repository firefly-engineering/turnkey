package mapper

import (
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// ReadVariant reads a target's variant attributes (attrs) as their values
// in each configuration of space: a select() is evaluated, and a list
// followed by a select() is concatenated with the branch that applies,
// each label once. An attribute the target doesn't set is absent from
// every configuration's variant. In a configuration no branch applies to,
// a list followed by a select() is the list alone, and a select() alone is
// absent. It reports
// false, with the attribute, if one can't be read: a select() with a key
// the space doesn't know, or a concatenation of values that aren't lists.
func ReadVariant(target *starlark.Target, attrs []string, space conditions.Space) (func(conditions.Configuration) map[string]starlark.AttributeValue, string, bool) {
	type conditional struct {
		sel     starlark.SelectValue
		matcher *conditions.Matcher
	}
	plain := make(map[string]starlark.AttributeValue)
	selects := make(map[string]conditional)
	for _, name := range attrs {
		a := target.GetAttribute(name)
		if a == nil {
			continue
		}
		sel, ok := a.Value.(starlark.SelectValue)
		if !ok {
			plain[name] = a.Value
			continue
		}
		keys := make([]string, len(sel.Branches))
		for i, b := range sel.Branches {
			keys[i] = b.Key
			if _, ok := starlark.Labels(b.Value); sel.Common != nil && !ok {
				return nil, name, false
			}
		}
		matcher, err := space.Matcher(keys)
		if err != nil {
			return nil, name, false
		}
		selects[name] = conditional{sel, matcher}
	}

	if len(plain) == 0 && len(selects) == 0 {
		return func(conditions.Configuration) map[string]starlark.AttributeValue { return nil }, "", true
	}
	return func(config conditions.Configuration) map[string]starlark.AttributeValue {
		variant := make(map[string]starlark.AttributeValue, len(plain)+len(selects))
		for name, value := range plain {
			variant[name] = value
		}
		for name, c := range selects {
			var value starlark.AttributeValue
			if i := c.matcher.Branch(config); i >= 0 {
				value = c.sel.Branches[i].Value
			}
			extra, ok := starlark.Labels(value)
			if !ok {
				variant[name] = value
				continue
			}
			if c.sel.Common == nil && value == nil {
				continue
			}
			common, _ := starlark.Labels(c.sel.Common)
			variant[name] = starlark.StringListValue{Values: dedupe(append(append([]string(nil), common...), extra...))}
		}
		return variant
	}, "", true
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
