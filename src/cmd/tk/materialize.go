package main

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/firefly-engineering/turnkey/src/go/pkg/materialize"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
)

// runMaterialize brings each deps cell named by a cell index in line with
// it (ADR 0004). The shell calls it with the indexes it built, where it
// keeps its other links current.
//
//	tk materialize <cell index>...
func runMaterialize(args []string) int {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "Usage: tk materialize <cell index>...")
		return 1
	}
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		return 1
	}
	status := 0
	for _, index := range args {
		result, err := materialize.Materialize(materialize.Options{
			Root:      root,
			IndexPath: index,
			AddRoot:   addGCRoot,
		})
		if err != nil {
			fmt.Fprintf(os.Stderr, "tk: materializing %s: %v\n", index, err)
			status = 1
			continue
		}
		if result.SwitchedOver && !quiet {
			fmt.Fprintf(os.Stderr, "tk: the deps cell for %s is now a real directory (to go back: rm -rf the cell)\n", index)
		}
		if (verbose || (!quiet && result.Changed())) && !result.SwitchedOver {
			fmt.Fprintf(os.Stderr, "tk: materialized %s: %d store links added, %d removed; %d packages written, %d removed\n",
				index, result.StoreLinksAdded, result.StoreLinksGone, result.PackagesWritten, result.PackagesGone)
		}
	}
	return status
}

// addGCRoot registers link as a GC root for storePath, through the Nix
// daemon: only it can write /nix/var/nix/gcroots. Nothing is built; the
// path is already in the store.
func addGCRoot(link, storePath string) error {
	cmd := exec.Command("nix-store", "--add-root", link, "--realise", storePath)
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("nix-store --add-root: %w\n%s", err, out)
	}
	return nil
}

// warnStaleCells warns when a materialized deps cell was built from another
// version of its deps file than the one on disk: the shell hasn't
// re-evaluated since the file changed.
func warnStaleCells() {
	root, err := findProjectRoot()
	if err != nil {
		return
	}
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		return
	}
	for _, lang := range cfg.Languages {
		if lang.Cell == "" || lang.DepsFile == "" {
			continue
		}
		stale, err := materialize.Stale(root, lang.Cell, lang.DepsFile)
		if err == nil && stale && !quiet {
			fmt.Fprintf(os.Stderr, "tk: warning: the %s cell was built from another %s; reload the shell (direnv reload) to rebuild it\n",
				lang.Cell, lang.DepsFile)
		}
	}
}
