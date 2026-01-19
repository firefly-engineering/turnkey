// godeps-gen generates go-deps.toml from go.mod and go.sum files.
//
// This tool parses Go module files and outputs dependency declarations
// in the format expected by turnkey's go-deps-cell.nix.
//
// Usage:
//
//	godeps-gen -o go-deps.toml
//	godeps-gen --go-mod go.mod --go-sum go.sum -o go-deps.toml
package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/firefly-engineering/turnkey/src/go/pkg/godeps"
)

func main() {
	goModPath := flag.String("go-mod", "go.mod", "path to go.mod file")
	goSumPath := flag.String("go-sum", "go.sum", "path to go.sum file")
	outputPath := flag.String("o", "", "output file path (default: stdout)")
	prefetch := flag.Bool("prefetch", false, "fetch Nix hashes using nix-prefetch-github (requires nix)")
	includeIndirect := flag.Bool("indirect", true, "include indirect (transitive) dependencies")
	flag.Parse()

	// Read go.mod
	goModData, err := os.ReadFile(*goModPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error reading go.mod: %v\n", err)
		os.Exit(1)
	}

	// Parse go.mod
	opts := godeps.ParseOptions{IncludeIndirect: *includeIndirect}
	deps, err := godeps.ParseGoMod(goModData, opts)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error parsing go.mod: %v\n", err)
		os.Exit(1)
	}

	// Read and parse go.sum
	goSumData, err := os.ReadFile(*goSumPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error reading go.sum: %v\n", err)
		os.Exit(1)
	}

	hashes, err := godeps.ParseGoSum(goSumData)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error parsing go.sum: %v\n", err)
		os.Exit(1)
	}

	// Merge hashes into dependencies
	godeps.MergeHashes(deps, hashes)

	// Prefetch Nix hashes if requested
	if *prefetch {
		fmt.Fprintf(os.Stderr, "Prefetching %d dependencies...\n", len(deps))
		prefetcher := godeps.DefaultPrefetcher(os.Stderr)
		godeps.PrefetchAll(deps, prefetcher, func(dep godeps.Dependency, err error) {
			fmt.Fprintf(os.Stderr, "warning: failed to prefetch %s: %v\n", dep.ImportPath, err)
		})
	}

	// Determine output destination
	var output io.Writer = os.Stdout
	if *outputPath != "" {
		f, err := os.Create(*outputPath)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error creating output file: %v\n", err)
			os.Exit(1)
		}
		defer func() { _ = f.Close() }()
		output = f
	}

	// Output TOML
	outputOpts := godeps.DefaultOutputOptions()
	if err := godeps.WriteTOML(output, deps, outputOpts); err != nil {
		fmt.Fprintf(os.Stderr, "error writing output: %v\n", err)
		os.Exit(1)
	}

	if *outputPath != "" {
		fmt.Fprintf(os.Stderr, "Wrote %s\n", *outputPath)
	}
}
