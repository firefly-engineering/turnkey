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
	fmt.Fprintf(w, "    srcs = glob([\"*.go\"]),\n")
	fmt.Fprintf(w, "    visibility = [\"PUBLIC\"],\n")

	// Dependencies
	fmt.Fprintf(w, "    %s = [\n", cfg.Buck.DepsAttr)
	for _, dep := range normalized.Common {
		if isStdLib(dep) {
			continue
		}
		// Skip self-references (package importing itself or parent)
		if dep == pkg.ImportPath || strings.HasPrefix(pkg.ImportPath, dep+"/") {
			continue
		}
		fmt.Fprintf(w, "        %q,\n", importToTarget(dep, cfg))
	}
	fmt.Fprintf(w, "    ]")

	// Platform-specific dependencies using select()
	if len(normalized.Platform) > 0 {
		fmt.Fprintln(w, " + select({")

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
			// Check if any non-stdlib deps
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
			if len(filteredDeps) == 0 {
				continue
			}

			constraint := findConstraint(p, cfg)
			fmt.Fprintf(w, "        %q: [\n", constraint)
			for _, d := range filteredDeps {
				fmt.Fprintf(w, "            %q,\n", d)
			}
			fmt.Fprintf(w, "        ],\n")
		}
		fmt.Fprintln(w, "        \"DEFAULT\": [],")
		fmt.Fprint(w, "    })")
	}
	fmt.Fprintln(w, ",")
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
		importPath := filepath.ToSlash(rel)

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
