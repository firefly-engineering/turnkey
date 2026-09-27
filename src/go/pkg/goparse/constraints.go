package goparse

import (
	"go/build/constraint"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// BuildContext is what a file's build constraints are evaluated against,
// as go/build evaluates them for a build.
type BuildContext struct {
	// GOOS and GOARCH are the target's, e.g. "linux" and "arm64".
	GOOS, GOARCH string

	// CgoEnabled is whether cgo is on: the cgo tag is set, and files
	// importing "C" are part of the build.
	CgoEnabled bool

	// GoVersion is the toolchain's Go version, e.g. "1.24": the release
	// tags go1.1 up to it are set.
	GoVersion string

	// Tags are the build tags set on the build (-tags).
	Tags []string
}

// unixOS are the GOOS values the unix tag is set for (go/build's list).
var unixOS = map[string]bool{
	"aix": true, "android": true, "darwin": true, "dragonfly": true, "freebsd": true,
	"hurd": true, "illumos": true, "ios": true, "linux": true, "netbsd": true,
	"openbsd": true, "solaris": true,
}

// Matches reports whether a file is part of the build: its //go:build
// constraint holds and its name's _GOOS/_GOARCH suffixes match.
func (ctx BuildContext) Matches(file *GoFile) bool {
	if file.HasCgo && !ctx.CgoEnabled {
		return false
	}
	if file.Constraint != nil && !file.Constraint.Eval(ctx.HasTag) {
		return false
	}
	osTag, archTag := ParseFilenameConstraint(filepath.Base(file.Path))
	if osTag != "" && !ctx.HasTag(osTag) {
		return false
	}
	if archTag != "" && archTag != ctx.GOARCH {
		return false
	}
	return true
}

// HasTag reports whether a build tag is set, as go/build decides: GOOS,
// GOARCH, unix on a Unix, cgo, gc (the compiler), release tags up to the
// Go version, and the build's own tags. android implies linux, and ios
// darwin.
func (ctx BuildContext) HasTag(tag string) bool {
	switch {
	case tag == ctx.GOOS || tag == ctx.GOARCH || tag == "gc":
		return true
	case tag == "unix":
		return unixOS[ctx.GOOS]
	case tag == "cgo":
		return ctx.CgoEnabled
	case tag == "linux" && ctx.GOOS == "android", tag == "darwin" && ctx.GOOS == "ios":
		return true
	}
	if minor, ok := releaseMinor(tag); ok {
		current, ok := releaseMinor("go" + ctx.GoVersion)
		return ok && minor <= current
	}
	for _, t := range ctx.Tags {
		if t == tag {
			return true
		}
	}
	return false
}

// releaseMinor returns N for a release tag go1.N (or a version go1.N.P).
func releaseMinor(tag string) (int, bool) {
	rest, ok := strings.CutPrefix(tag, "go1.")
	if !ok {
		return 0, false
	}
	if dot := strings.IndexByte(rest, '.'); dot >= 0 {
		rest = rest[:dot]
	}
	minor, err := strconv.Atoi(rest)
	return minor, err == nil
}

// ConstraintTags returns the tags a file's build constraint names, sorted.
func (f *GoFile) ConstraintTags() []string {
	seen := make(map[string]bool)
	var walk func(constraint.Expr)
	walk = func(e constraint.Expr) {
		switch e := e.(type) {
		case *constraint.TagExpr:
			seen[e.Tag] = true
		case *constraint.NotExpr:
			walk(e.X)
		case *constraint.AndExpr:
			walk(e.X)
			walk(e.Y)
		case *constraint.OrExpr:
			walk(e.X)
			walk(e.Y)
		}
	}
	if f.Constraint != nil {
		walk(f.Constraint)
	}
	tags := make([]string, 0, len(seen))
	for tag := range seen {
		tags = append(tags, tag)
	}
	sort.Strings(tags)
	return tags
}

var knownOS = map[string]bool{
	"aix":       true,
	"android":   true,
	"darwin":    true,
	"dragonfly": true,
	"freebsd":   true,
	"hurd":      true,
	"illumos":   true,
	"ios":       true,
	"js":        true,
	"linux":     true,
	"nacl":      true,
	"netbsd":    true,
	"openbsd":   true,
	"plan9":     true,
	"solaris":   true,
	"windows":   true,
	"zos":       true,
}

var knownArch = map[string]bool{
	"386":         true,
	"amd64":       true,
	"amd64p32":    true,
	"arm":         true,
	"armbe":       true,
	"arm64":       true,
	"arm64be":     true,
	"ppc64":       true,
	"ppc64le":     true,
	"mips":        true,
	"mipsle":      true,
	"mips64":      true,
	"mips64le":    true,
	"mips64p32":   true,
	"mips64p32le": true,
	"ppc":         true,
	"riscv":       true,
	"riscv64":     true,
	"s390":        true,
	"s390x":       true,
	"sparc":       true,
	"sparc64":     true,
	"wasm":        true,
}

// ParseFilenameConstraint extracts OS/arch constraints from filename.
func ParseFilenameConstraint(filename string) (os, arch string) {
	name := strings.TrimSuffix(filename, ".go")
	name = strings.TrimSuffix(name, "_test")

	parts := strings.Split(name, "_")
	if len(parts) < 2 {
		return "", ""
	}

	// Check last two parts for _GOOS_GOARCH
	if len(parts) >= 3 {
		last := parts[len(parts)-1]
		prev := parts[len(parts)-2]
		if knownOS[prev] && knownArch[last] {
			return prev, last
		}
	}

	// Check last part for _GOOS or _GOARCH
	last := parts[len(parts)-1]
	if knownOS[last] {
		return last, ""
	}
	if knownArch[last] {
		return "", last
	}

	return "", ""
}
