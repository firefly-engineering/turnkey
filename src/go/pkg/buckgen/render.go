package buckgen

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
)

// RenderPackage generates a rules.star file for a Go package
func RenderPackage(w io.Writer, pkg *goparse.GoPackage, cfg *Config) error {
	if cfg == nil {
		cfg = DefaultConfig()
	}

	if cfg.Buck.Preambule != "" {
		fmt.Fprintln(w, cfg.Buck.Preambule)
		fmt.Fprintln(w)
	}

	normalized := NormalizeDeps(pkg.Imports)

	// Use directory name (last component of import path) as target name for consistency.
	// This ensures deps can reference targets without knowing the Go package name.
	// e.g., github.com/pelletier/go-toml/v2 -> target name "v2"
	targetName := filepath.Base(pkg.ImportPath)

	fmt.Fprintf(w, "%s(\n", cfg.Buck.GoLibraryRule)
	fmt.Fprintf(w, "    name = %q,\n", targetName)
	fmt.Fprintf(w, "    package_name = %q,\n", pkg.ImportPath)
	// Include Go, assembly, and C/C++ source files that may be part of Go packages
	fmt.Fprintf(w, "    srcs = native.glob([\"*.go\", \"*.s\", \"*.h\", \"*.c\", \"*.cc\", \"*.cpp\", \"*.S\"]),\n")
	fmt.Fprintf(w, "    header_namespace = \"\",\n")
	fmt.Fprintf(w, "    visibility = [\"PUBLIC\"],\n")

	// Collect all dependencies (common + platform-specific)
	var commonDeps []string
	for _, dep := range normalized.Common {
		if isStdLib(dep) {
			continue
		}
		// Skip self-references (package importing itself or parent)
		if dep == pkg.ImportPath || strings.HasPrefix(pkg.ImportPath, dep+"/") {
			continue
		}
		commonDeps = append(commonDeps, importToTarget(dep, cfg))
	}

	// Collect platform-specific dependencies
	var platformDeps []struct {
		constraint string
		deps       []string
	}
	if len(normalized.Platform) > 0 {
		// Sort platforms for deterministic output
		var platforms []goparse.Platform
		for p := range normalized.Platform {
			platforms = append(platforms, p)
		}
		sort.Slice(platforms, func(i, j int) bool {
			if platforms[i].OS != platforms[j].OS {
				return platforms[i].OS < platforms[j].OS
			}
			return platforms[i].Arch < platforms[j].Arch
		})

		for _, p := range platforms {
			deps := normalized.Platform[p]
			var filteredDeps []string
			for _, d := range deps {
				if isStdLib(d) {
					continue
				}
				// Skip self-references
				if d == pkg.ImportPath || strings.HasPrefix(pkg.ImportPath, d+"/") {
					continue
				}
				filteredDeps = append(filteredDeps, importToTarget(d, cfg))
			}
			if len(filteredDeps) > 0 {
				constraint := findConstraint(p, cfg)
				platformDeps = append(platformDeps, struct {
					constraint string
					deps       []string
				}{constraint, filteredDeps})
			}
		}
	}

	// Only output deps attribute if there are actual dependencies
	if len(commonDeps) > 0 || len(platformDeps) > 0 {
		fmt.Fprintf(w, "    %s = [\n", cfg.Buck.DepsAttr)
		for _, dep := range commonDeps {
			fmt.Fprintf(w, "        %q,\n", dep)
		}
		fmt.Fprintf(w, "    ]")

		if len(platformDeps) > 0 {
			fmt.Fprintln(w, " + select({")
			for _, pd := range platformDeps {
				fmt.Fprintf(w, "        %q: [\n", pd.constraint)
				for _, d := range pd.deps {
					fmt.Fprintf(w, "            %q,\n", d)
				}
				fmt.Fprintf(w, "        ],\n")
			}
			fmt.Fprintln(w, "        \"DEFAULT\": [],")
			fmt.Fprint(w, "    })")
		}
		fmt.Fprintln(w, ",")
	}
	fmt.Fprintln(w, ")")

	return nil
}

// RenderCell generates rules.star files for all packages in a vendor directory
func RenderCell(vendorDir string, cfg *Config) ([]string, error) {
	if cfg == nil {
		cfg = DefaultConfig()
	}

	absVendor, err := filepath.Abs(vendorDir)
	if err != nil {
		return nil, err
	}

	var generated []string

	err = filepath.Walk(absVendor, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if !info.IsDir() {
			return nil
		}

		// Skip hidden directories
		if strings.HasPrefix(info.Name(), ".") {
			return filepath.SkipDir
		}

		// Calculate import path relative to vendorDir
		rel, err := filepath.Rel(absVendor, path)
		if err != nil {
			return nil
		}
		if rel == "." {
			return nil
		}
		// Strip @version suffixes from path components
		// e.g., "golang.org/x/mod@v0.31.0/module" -> "golang.org/x/mod/module"
		importPath := stripVersionsFromPath(filepath.ToSlash(rel))

		// Try to parse as a Go package
		pkg, err := goparse.ScanPackage(path, importPath, cfg.PlatformsToGoparse())
		if err != nil || pkg == nil {
			// Not a go package or other error, just skip
			return nil
		}

		// If it has no Go files, skip
		if len(pkg.GoFiles) == 0 {
			return nil
		}

		// Generate rules.star
		buildFile := filepath.Join(path, cfg.Buck.BuildfileName)
		f, err := os.Create(buildFile)
		if err != nil {
			return err
		}
		defer f.Close()

		if err := RenderPackage(f, pkg, cfg); err != nil {
			return err
		}

		generated = append(generated, buildFile)
		return nil
	})

	return generated, err
}

func isStdLib(importPath string) bool {
	if importPath == "C" {
		return true
	}
	parts := strings.Split(importPath, "/")
	return !strings.Contains(parts[0], ".")
}

// stripVersionsFromPath removes @version suffixes from path components.
// e.g., "golang.org/x/mod@v0.31.0/module" -> "golang.org/x/mod/module"
func stripVersionsFromPath(path string) string {
	parts := strings.Split(path, "/")
	for i, part := range parts {
		if idx := strings.Index(part, "@"); idx != -1 {
			parts[i] = part[:idx]
		}
	}
	return strings.Join(parts, "/")
}

func importToTarget(importPath string, cfg *Config) string {
	// Use directory name (last path component) as target name.
	// This matches the target name generation in RenderPackage.
	parts := strings.Split(importPath, "/")
	name := parts[len(parts)-1]
	return fmt.Sprintf("%s%s:%s", cfg.Buck.DepsTargetLabelPrefix, importPath, name)
}

func findConstraint(p goparse.Platform, cfg *Config) string {
	for _, pc := range cfg.Platforms {
		if pc.GoOS == p.OS && pc.GoArch == p.Arch {
			// Prefer OS constraint if available
			if pc.BuckOS != "" {
				return cfg.Buck.OSConstraintPrefix + pc.BuckOS
			}
		}
	}
	return "DEFAULT"
}

// Helper to convert buckgen.Platform to goparse.Platform
func (c *Config) PlatformsToGoparse() []goparse.Platform {
	res := make([]goparse.Platform, len(c.Platforms))
	for i, p := range c.Platforms {
		res[i] = goparse.Platform{OS: p.GoOS, Arch: p.GoArch}
	}
	return res
}
