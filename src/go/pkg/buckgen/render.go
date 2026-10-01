package buckgen

import (
	"fmt"
	"io"
	"io/fs"
	"maps"
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
	_, built, err := renderPackage(w, pkg, cfg)
	return built, err
}

// renderPackage is RenderPackage, also returning the non-stdlib import
// paths its deps reference, in any configuration.
func renderPackage(w io.Writer, pkg *goparse.GoPackage, cfg *Config) ([]string, bool, error) {
	space := packageSpace(pkg, cfg)
	deps := make(map[string][]string, len(space.Configurations))
	referenced := map[string]bool{}
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
			referenced[imp] = true
		}
		deps[config.String()] = targets
	}
	if !built {
		return nil, false, nil
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

	imports := slices.Sorted(maps.Keys(referenced))
	return imports, true, nil
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

// A RenderedPackage is a Go package RenderModule wrote a rules.star for:
// its directory in the module ("." for the module's root), and its target.
type RenderedPackage struct {
	Subdir string
	Target string
}

// RenderModule generates a rules.star file for each Go package of the
// module in moduleDir, whose module path is modulePath, as the go command
// sees its packages: directories named testdata, or starting with "." or
// "_", hold none. It returns the packages it rendered, by directory, and
// the non-stdlib import paths their deps reference.
func RenderModule(moduleDir, modulePath string, cfg *Config) ([]RenderedPackage, []string, error) {
	var rendered []RenderedPackage
	referenced := map[string]bool{}
	err := filepath.WalkDir(moduleDir, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(moduleDir, path)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		importPath := modulePath
		if rel != "." {
			name := d.Name()
			if name == "testdata" || strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_") {
				return filepath.SkipDir
			}
			importPath += "/" + rel
		}

		pkg, err := goparse.ScanPackage(path, importPath)
		if err != nil || pkg == nil {
			// Not a Go package
			return nil
		}

		// Generate rules.star, unless no configuration builds the package
		var content strings.Builder
		imports, built, err := renderPackage(&content, pkg, cfg)
		if err != nil {
			return err
		}
		if !built {
			return nil
		}
		if err := os.WriteFile(filepath.Join(path, cfg.Buck.BuildfileName), []byte(content.String()), 0o644); err != nil {
			return err
		}
		rendered = append(rendered, RenderedPackage{Subdir: rel, Target: filepath.Base(importPath)})
		for _, imp := range imports {
			referenced[imp] = true
		}
		return nil
	})
	if err != nil {
		return nil, nil, err
	}
	return rendered, slices.Sorted(maps.Keys(referenced)), nil
}

func isStdLib(importPath string) bool {
	if importPath == "C" {
		return true
	}
	parts := strings.Split(importPath, "/")
	return !strings.Contains(parts[0], ".")
}

// importToTarget is the label of an imported package in the deps cell:
// its directory name (the import path's last component) is its target
// name, as RenderPackage names it.
func importToTarget(importPath string, cfg *Config) string {
	parts := strings.Split(importPath, "/")
	name := parts[len(parts)-1]
	return fmt.Sprintf("%s%s:%s", cfg.Buck.DepsTargetLabelPrefix, importPath, name)
}
