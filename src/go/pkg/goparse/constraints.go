package goparse

import (
	"path/filepath"
	"strings"
)

// MatchesPlatform checks if a file should be included for the given platform.
// Evaluates both:
// 1. //go:build constraints from file content
// 2. Filename conventions (*_linux.go, *_amd64.go, etc.)
func MatchesPlatform(file *GoFile, platform Platform) bool {
	// 1. Evaluate //go:build
	if file.Constraint != nil {
		if !file.Constraint.Eval(func(tag string) bool {
			// We only care about OS and Arch tags for now.
			// Other tags (like "cgo") could be handled too if needed.
			if tag == platform.OS || tag == platform.Arch {
				return true
			}
			if tag == "cgo" && file.HasCgo {
				return true
			}
			return false
		}) {
			return false
		}
	}

	// 2. Evaluate filename
	osTag, archTag := ParseFilenameConstraint(filepath.Base(file.Path))
	if osTag != "" && osTag != platform.OS {
		return false
	}
	if archTag != "" && archTag != platform.Arch {
		return false
	}

	return true
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
