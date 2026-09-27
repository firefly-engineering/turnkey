package goparse

import (
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// tagDimensionPrefix starts the name of a Go build tag's configuration
// dimension, e.g. "go_tag:integration".
const tagDimensionPrefix = "go_tag:"

// TagDimension returns the configuration dimension of a Go build tag: on
// when the tag is set, backed by the prelude's
// prelude//go/tags/constraints:<tag> constraint.
func TagDimension(tag string) conditions.OnOff {
	return conditions.OnOff{
		Name:       tagDimensionPrefix + tag,
		Constraint: "prelude//go/tags/constraints:" + tag,
		Token:      tag,
	}
}

// ConfigTags returns the build tags a configuration sets, sorted: those
// whose dimension (TagDimension) it sets on.
func ConfigTags(config conditions.Configuration) []string {
	var tags []string
	for dim, value := range config {
		if tag, ok := strings.CutPrefix(dim, tagDimensionPrefix); ok && value == conditions.Set {
			tags = append(tags, tag)
		}
	}
	sort.Strings(tags)
	return tags
}
