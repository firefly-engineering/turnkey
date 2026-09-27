package mapper

import (
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditional"
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// ReadVariant reads a target's variant attributes (attrs) as their values
// in each configuration of space, as conditional.Read reads them. An
// attribute with no value in a configuration (unset, or a select() alone
// with no branch for it) is absent from that configuration's variant. It
// reports false, with the attribute, if one can't be read.
func ReadVariant(target *starlark.Target, attrs []string, space conditions.Space) (func(conditions.Configuration) map[string]starlark.AttributeValue, string, bool) {
	values := make(map[string]func(conditions.Configuration) starlark.AttributeValue, len(attrs))
	for _, name := range attrs {
		read, err := conditional.Read(target, name, space)
		if err != nil {
			return nil, name, false
		}
		values[name] = read
	}
	return func(config conditions.Configuration) map[string]starlark.AttributeValue {
		var variant map[string]starlark.AttributeValue
		for name, read := range values {
			if value := read(config); value != nil {
				if variant == nil {
					variant = make(map[string]starlark.AttributeValue, len(values))
				}
				variant[name] = value
			}
		}
		return variant
	}, "", true
}
