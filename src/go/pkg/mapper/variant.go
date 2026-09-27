package mapper

import (
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// ReadVariant reads a target's variant attributes (attrs) as their values
// in each configuration of space: a select() is evaluated, and a list
// followed by a select() is concatenated with the branch that applies. An
// attribute the target doesn't set, or whose select() has no branch for a
// configuration, is absent from that configuration's variant. It reports
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
			i := c.matcher.Branch(config)
			if i < 0 {
				continue
			}
			value := c.sel.Branches[i].Value
			if c.sel.Common != nil {
				common, _ := starlark.Labels(c.sel.Common)
				extra, _ := starlark.Labels(value)
				value = starlark.StringListValue{Values: append(append([]string(nil), common...), extra...)}
			}
			variant[name] = value
		}
		return variant
	}, "", true
}
