// pydeps-cell writes the rules.star files of the pydeps cell's vendored
// packages, with the dependencies between them evaluated per platform
// (src/go/pkg/pydepscell).
//
// Usage: pydeps-cell <cell-dir> <python-deps.toml>, with the platforms
// and the Python version in <cell-dir>/pydeps-cell.json.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/pydepscell"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "Usage: pydeps-cell <cell-dir> <python-deps.toml>")
		os.Exit(1)
	}
	cellDir, depsPath := os.Args[1], os.Args[2]

	data, err := os.ReadFile(filepath.Join(cellDir, "pydeps-cell.json"))
	if err != nil {
		fmt.Fprintf(os.Stderr, "pydeps-cell: %v\n", err)
		os.Exit(1)
	}
	var cfg pydepscell.Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		fmt.Fprintf(os.Stderr, "pydeps-cell: parsing pydeps-cell.json: %v\n", err)
		os.Exit(1)
	}
	deps, err := pydepscell.LoadDeps(depsPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "pydeps-cell: %v\n", err)
		os.Exit(1)
	}
	if err := pydepscell.RenderCell(cellDir, deps, cfg); err != nil {
		fmt.Fprintf(os.Stderr, "pydeps-cell: %v\n", err)
		os.Exit(1)
	}
}
