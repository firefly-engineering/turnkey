// buckgen generates the rules.star files of one Go module, in place.
//
// Usage:
//
//	buckgen --config <buckgen.json> --module-path <module path> \
//	  --targets-out <file> --imports-out <file> <module-dir>
//
// It writes, one per line, each Go package it rendered as "<subdir>
// <target>" ("." for the module's root) to --targets-out, and the non-stdlib
// import paths their deps reference to --imports-out.
package main

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/buckgen"
)

func main() {
	configPath := flag.String("config", "", "buckgen configuration (JSON)")
	modulePath := flag.String("module-path", "", "the module's path (its go.mod's module line)")
	targetsOut := flag.String("targets-out", "", "where to list the rendered packages")
	importsOut := flag.String("imports-out", "", "where to list the import paths their deps reference")
	flag.Parse()
	if flag.NArg() != 1 || *configPath == "" || *modulePath == "" || *targetsOut == "" || *importsOut == "" {
		fmt.Fprintln(os.Stderr, "Usage: buckgen --config <json> --module-path <path> --targets-out <file> --imports-out <file> <module-dir>")
		os.Exit(2)
	}

	cfg, err := buckgen.LoadConfig(*configPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error loading config: %v\n", err)
		os.Exit(1)
	}

	rendered, imports, err := buckgen.RenderModule(flag.Arg(0), *modulePath, cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error rendering %s: %v\n", *modulePath, err)
		os.Exit(1)
	}

	var targets strings.Builder
	for _, pkg := range rendered {
		fmt.Fprintf(&targets, "%s %s\n", pkg.Subdir, pkg.Target)
	}
	if err := os.WriteFile(*targetsOut, []byte(targets.String()), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "Error writing %s: %v\n", *targetsOut, err)
		os.Exit(1)
	}
	var lines strings.Builder
	for _, imp := range imports {
		lines.WriteString(imp + "\n")
	}
	if err := os.WriteFile(*importsOut, []byte(lines.String()), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "Error writing %s: %v\n", *importsOut, err)
		os.Exit(1)
	}
}
