package buckgen

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditional"
	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
	"github.com/firefly-engineering/turnkey/src/go/pkg/starlark"
)

// RenderPackage generates a rules.star file for a Go package. Its deps are
// resolved in every configuration: on each platform, and for each
// combination of the allowed build tags its files' constraints use. Deps
// every configuration has are a plain list; the others are a select()
// keyed as the conditions core keys them, with no DEFAULT. It reports
// false, writing nothing, if no configuration builds the package (e.g. a
// Windows-only package).
func RenderPackage(w io.Writer, pkg *goparse.GoPackage, cfg *Config) (bool, error) {
	space := packageSpace(pkg, cfg)
	deps := make(map[string][]string, len(space.Configurations))
	built := false
	for _, config := range space.Configurations {
		imports, ok := pkg.Imports(buildContext(config, cfg))
		built = built || ok
		var targets []string
		for _, imp := range imports {
			if isStdLib(imp) {
				continue
			}
			// Skip self-references (package importing itself or parent)
			if imp == pkg.ImportPath || strings.HasPrefix(pkg.ImportPath, imp+"/") {
				continue
			}
			targets = append(targets, importToTarget(imp, cfg))
		}
		deps[config.String()] = targets
	}
	if !built {
		return false, nil
	}

	if cfg.Buck.Preambule != "" {
		fmt.Fprintln(w, cfg.Buck.Preambule)
		fmt.Fprintln(w)
	}

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

	value := conditional.LabelsValue(space, func(config conditions.Configuration) []string { return deps[config.String()] })
	// Only output deps attribute if there are actual dependencies
	if value != nil {
		fmt.Fprintf(w, "    %s = %s,\n", cfg.Buck.DepsAttr, starlark.RenderIndented(value, "    "))
	}
	fmt.Fprintln(w, ")")

	return true, nil
}

// packageSpace returns the configurations a package's deps are resolved
// for: every platform, crossed with each allowed build tag its files'
// constraints use.
func packageSpace(pkg *goparse.GoPackage, cfg *Config) conditions.Space {
	var dims []conditions.OnOff
	for _, f := range pkg.Files {
		for _, tag := range f.ConstraintTags() {
			if slices.Contains(cfg.Conditions.GoTags, tag) {
				dims = append(dims, goparse.TagDimension(tag))
			}
		}
	}
	return conditions.NewSpace(cfg.Conditions.Platforms, cfg.Conditions.Settings).WithDimensions(dims)
}

// buildContext returns the Go build of a configuration (goparse's), with
// the toolchain's release tags.
func buildContext(config conditions.Configuration, cfg *Config) goparse.BuildContext {
	ctx, _ := goparse.ConfigContext(config)
	ctx.GoVersion = cfg.GoVersion
	return ctx
}

// RenderCell generates rules.star files for all packages in a vendor directory
func RenderCell(vendorDir string, cfg *Config) ([]string, error) {
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
		pkg, err := goparse.ScanPackage(path, importPath)
		if err != nil || pkg == nil {
			// Not a go package or other error, just skip
			return nil
		}

		// Generate rules.star, unless no configuration builds the package
		var content strings.Builder
		built, err := RenderPackage(&content, pkg, cfg)
		if err != nil {
			return err
		}
		if !built {
			return nil
		}
		buildFile := filepath.Join(path, cfg.Buck.BuildfileName)
		if err := os.WriteFile(buildFile, []byte(content.String()), 0o644); err != nil {
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
	// Check if this import path has a local replacement
	if cfg.LocalReplaces != nil {
		if target, ok := cfg.LocalReplaces[importPath]; ok {
			return target
		}
		// Also check for prefix matches (e.g., github.com/foo/bar/pkg matches github.com/foo/bar)
		for replacePrefix, target := range cfg.LocalReplaces {
			if strings.HasPrefix(importPath, replacePrefix+"/") {
				// Append the subpath to the target
				// e.g., "//src/mylib:mylib" + "/subpkg" -> "//src/mylib/subpkg:subpkg"
				subpath := strings.TrimPrefix(importPath, replacePrefix)
				subpkgName := filepath.Base(importPath)
				// Extract the cell/path part from target (e.g., "//src/mylib" from "//src/mylib:mylib")
				colonIdx := strings.LastIndex(target, ":")
				if colonIdx != -1 {
					basePath := target[:colonIdx]
					return fmt.Sprintf("%s%s:%s", basePath, subpath, subpkgName)
				}
				return fmt.Sprintf("%s%s:%s", target, subpath, subpkgName)
			}
		}
	}

	// Use directory name (last path component) as target name.
	// This matches the target name generation in RenderPackage.
	parts := strings.Split(importPath, "/")
	name := parts[len(parts)-1]
	return fmt.Sprintf("%s%s:%s", cfg.Buck.DepsTargetLabelPrefix, importPath, name)
}
