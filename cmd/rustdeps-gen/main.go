// rustdeps-gen generates rust-deps.toml from Cargo.lock files.
//
// This tool parses Cargo.lock files and outputs dependency declarations
// in the format expected by turnkey's rust-deps-cell.nix.
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
	prefetch := flag.Bool("prefetch", false, "fetch Nix hashes using nix-prefetch-url (fallback for missing checksums)")
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

	// Convert Cargo.lock checksums to Nix SRI hashes
	// The checksum in Cargo.lock is the SHA256 of the crate tarball
	if err := rustdeps.PopulateNixHashes(crates); err != nil {
		fmt.Fprintf(os.Stderr, "error converting checksums: %v\n", err)
		os.Exit(1)
	}

	// Prefetch missing hashes if requested
	if *prefetch {
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
