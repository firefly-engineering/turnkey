// godeps-gen generates go-deps.toml from a Go workspace: the members of
// the project's go.work, or, without one, its go.mod (ADR 0007).
//
// The modules are those the members require, minus the members; their
// versions are the build list `go list -m -json all` selects for the whole
// workspace. go-deps.toml records the members and the files it was
// generated from (go.work, go.work.sum, each member's go.mod and go.sum).
// This tool outputs dependency declarations in the format expected by
// turnkey's Go deps cell (nix/buck2/languages.nix).
//
// Its --output, --no-prefetch and --no-cache flags are the ones every deps
// generator takes (src/rust/deps-gen-kit): prefetching is on by default.
// tk sync runs it from the go sync rule (nix/buck2/languages.nix), in the
// project root.
//
// Usage:
//
//	godeps-gen -o go-deps.toml
//	godeps-gen --go-work go.work --go-mod go.mod --go-sum go.sum -o go-deps.toml
package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/firefly-engineering/turnkey/src/go/pkg/godeps"
)

func main() {
	goWorkPath := flag.String("go-work", "go.work", "path to go.work; when it exists, its members are the workspace and --go-mod and --go-sum are unused")
	goModPath := flag.String("go-mod", "go.mod", "path to go.mod file, the only member without a go.work")
	goSumPath := flag.String("go-sum", "go.sum", "path to go.sum file, with --go-mod")
	var outputPath string
	flag.StringVar(&outputPath, "o", "", "output file path (default: stdout)")
	flag.StringVar(&outputPath, "output", "", "output file path (default: stdout)")
	noPrefetch := flag.Bool("no-prefetch", false, "skip fetching the Nix hashes of the modules' proxy.golang.org zips (the deps file gets go.sum hashes Nix can't fetch with)")
	noCache := flag.Bool("no-cache", false, "always fetch from the network, bypassing turnkey's prefetch cache")
	includeIndirect := flag.Bool("indirect", true, "include indirect (transitive) dependencies")
	flag.Parse()

	if *noPrefetch && *noCache {
		fmt.Fprintln(os.Stderr, "error: --no-prefetch cannot be used with --no-cache")
		os.Exit(2)
	}

	// File paths are relative to the working directory, the project root
	root, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}

	ws, err := godeps.LoadWorkspace(root, *goWorkPath, *goModPath, *goSumPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error reading the Go workspace: %v\n", err)
		os.Exit(1)
	}

	lister := godeps.GoLister{Go: "go", Env: os.Environ()}
	opts := godeps.ParseOptions{IncludeIndirect: *includeIndirect}
	deps, err := ws.Resolve(root, lister, opts)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving the Go workspace: %v\n", err)
		os.Exit(1)
	}

	// Prefetch Nix hashes unless asked not to
	if !*noPrefetch {
		fmt.Fprintf(os.Stderr, "Prefetching %d dependencies...\n", len(deps))

		prefetcher := godeps.DefaultPrefetcher(os.Stderr, *noCache)
		godeps.PrefetchAll(deps, prefetcher, func(dep godeps.Dependency, err error) {
			fmt.Fprintf(os.Stderr, "warning: failed to prefetch %s: %v\n", dep.ImportPath, err)
		})
	}

	// Determine output destination
	var output io.Writer = os.Stdout
	if outputPath != "" {
		f, err := os.Create(outputPath)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error creating output file: %v\n", err)
			os.Exit(1)
		}
		defer func() { _ = f.Close() }()
		output = f
	}

	file := godeps.DepsFile{Deps: deps, Sources: ws.Sources, Members: ws.Members}
	if err := godeps.WriteDepsFile(output, file, godeps.DefaultOutputOptions()); err != nil {
		fmt.Fprintf(os.Stderr, "error writing output: %v\n", err)
		os.Exit(1)
	}

	if outputPath != "" {
		fmt.Fprintf(os.Stderr, "Wrote %s\n", outputPath)
	}
}
