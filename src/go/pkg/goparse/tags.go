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

// Go's names for Buck2's os and cpu constraint values
var (
	goOS   = map[string]string{"linux": "linux", "macos": "darwin"}
	goArch = map[string]string{"x86_64": "amd64", "arm64": "arm64"}
)

// ConfigContext returns the Go build of a configuration: its platform's
// GOOS and GOARCH, cgo, and the tags it sets (ConfigTags). GoVersion is
// left to the caller, whose toolchain it is. It reports false, with GOOS
// and GOARCH empty, for a configuration without a platform Go has names
// for.
func ConfigContext(config conditions.Configuration) (BuildContext, bool) {
	ctx := BuildContext{CgoEnabled: true, Tags: ConfigTags(config)}
	goos, goarch := goOS[config[conditions.OS]], goArch[config[conditions.CPU]]
	if goos == "" || goarch == "" {
		return ctx, false
	}
	ctx.GOOS, ctx.GOARCH = goos, goarch
	return ctx, true
}

// Environ returns the environment the go command needs to build for ctx's
// platform, whatever the host: GOOS, GOARCH and cgo. It is empty for a
// context without a platform, which the go command takes as the host's.
func (ctx BuildContext) Environ() []string {
	if ctx.GOOS == "" || ctx.GOARCH == "" {
		return nil
	}
	cgo := "0"
	if ctx.CgoEnabled {
		cgo = "1"
	}
	return []string{"GOOS=" + ctx.GOOS, "GOARCH=" + ctx.GOARCH, "CGO_ENABLED=" + cgo}
}
