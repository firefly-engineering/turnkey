package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/buckgen"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "Usage: buckgen <cell-dir>")
		os.Exit(1)
	}

	cellDir := os.Args[1]

	// Load buckgen.json config
	configPath := filepath.Join(cellDir, "buckgen.json")
	cfg, err := buckgen.LoadConfig(configPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error loading config: %v\n", err)
		os.Exit(1)
	}

	// Render all packages in vendor/
	vendorDir := filepath.Join(cellDir, "vendor")
	files, err := buckgen.RenderCell(vendorDir, cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error rendering cell: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Generated %d rules.star files\n", len(files))
}
