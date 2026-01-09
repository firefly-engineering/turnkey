// godeps-gen generates go-deps.toml from go.mod and go.sum files.
//
// This tool parses Go module files and outputs dependency declarations
// in the format expected by turnkey's go-deps-cell.nix.
//
// Usage:
//
//	godeps-gen --go-mod go.mod --go-sum go.sum > go-deps.toml
package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/firefly-engineering/turnkey/go/pkg/godeps"
)

func main() {
	goModPath := flag.String("go-mod", "go.mod", "path to go.mod file")
	goSumPath := flag.String("go-sum", "go.sum", "path to go.sum file")
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
		prefetcher := godeps.DefaultPrefetcher(os.Stderr)
		godeps.PrefetchAll(deps, prefetcher, func(dep godeps.Dependency, err error) {
			fmt.Fprintf(os.Stderr, "warning: failed to prefetch %s: %v\n", dep.ImportPath, err)
		})
	}

	// Output TOML
	outputOpts := godeps.DefaultOutputOptions()
	if err := godeps.WriteTOML(os.Stdout, deps, outputOpts); err != nil {
		fmt.Fprintf(os.Stderr, "error writing output: %v\n", err)
		os.Exit(1)
	}
}
