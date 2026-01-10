// rustdeps-gen generates rust-deps.toml from Cargo.lock files.
//
// This tool parses Cargo.lock files and outputs dependency declarations
// in the format expected by turnkey's rust-deps-cell.nix.
//
// IMPORTANT: The checksums in Cargo.lock are for the .crate tarball, but
// Nix's fetchzip computes hashes of the unpacked contents. Therefore,
// this tool must prefetch each crate to get the correct Nix hash.
//
// Usage:
//
//	rustdeps-gen --cargo-lock Cargo.lock > rust-deps.toml
package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/firefly-engineering/turnkey/go/pkg/rustdeps"
)

func main() {
	cargoLockPath := flag.String("cargo-lock", "Cargo.lock", "path to Cargo.lock file")
	noPrefetch := flag.Bool("no-prefetch", false, "skip prefetching (output will have incorrect hashes)")
	flag.Parse()

	// Read Cargo.lock
	cargoLockData, err := os.ReadFile(*cargoLockPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error reading Cargo.lock: %v\n", err)
		os.Exit(1)
	}

	// Parse Cargo.lock
	opts := rustdeps.DefaultParseOptions()
	crates, err := rustdeps.ParseCargoLock(cargoLockData, opts)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error parsing Cargo.lock: %v\n", err)
		os.Exit(1)
	}

	if *noPrefetch {
		// Use Cargo.lock checksums directly (WARNING: these are wrong for fetchzip!)
		// This is only useful for testing or if using fetchurl instead of fetchzip
		fmt.Fprintf(os.Stderr, "WARNING: --no-prefetch produces incorrect hashes for fetchzip\n")
		fmt.Fprintf(os.Stderr, "The Cargo.lock checksum is for the tarball, not unpacked contents\n")
		if err := rustdeps.PopulateNixHashes(crates); err != nil {
			fmt.Fprintf(os.Stderr, "error converting checksums: %v\n", err)
			os.Exit(1)
		}
	} else {
		// Prefetch all crates to get correct Nix hashes
		fmt.Fprintf(os.Stderr, "Prefetching %d crates from crates.io...\n", len(crates))
		prefetcher := rustdeps.DefaultPrefetcher(os.Stderr)
		rustdeps.PrefetchAll(crates, prefetcher, func(crate rustdeps.Crate, err error) {
			fmt.Fprintf(os.Stderr, "warning: failed to prefetch %s: %v\n", crate.Name, err)
		})
	}

	// Output TOML
	outputOpts := rustdeps.DefaultOutputOptions()
	if err := rustdeps.WriteTOML(os.Stdout, crates, outputOpts); err != nil {
		fmt.Fprintf(os.Stderr, "error writing output: %v\n", err)
		os.Exit(1)
	}
}
