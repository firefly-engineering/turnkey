// rules-sync keeps the deps of a project's rules.star files in step with
// their sources (src/go/pkg/rulessync), and prints what it did as a JSON
// report (src/go/pkg/rulesreport) on stdout.
//
// tk runs it for `tk rules check`, `tk rules sync` and the rules sync
// before a buck2 command, and prints the report its own way. It is a
// separate binary so that rules sync can be ported to Rust and switched
// over on its own; once tk is Rust, rules sync goes back into tk.
//
// Its verbose messages go to stderr. It exits 0 when sync ran, 1 when an
// error stopped it (the report's error says where), and 2 on bad usage.
//
// Usage:
//
//	rules-sync --project-root <root> [--dry-run] [--verbose] [--force] [dir]
//
// dir is the directory whose rules.star files are synced, the project root
// by default.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rulesreport"
	"github.com/firefly-engineering/turnkey/src/go/pkg/rulessync"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// run runs rules-sync with the command-line args, and returns its exit code.
func run(args []string, stdout, stderr io.Writer) int {
	fs := flag.NewFlagSet("rules-sync", flag.ContinueOnError)
	fs.SetOutput(stderr)
	fs.Usage = func() {
		fmt.Fprintln(stderr, "Usage: rules-sync --project-root <root> [--dry-run] [--verbose] [--force] [dir]")
		fs.PrintDefaults()
	}
	var cfg rulessync.Config
	fs.StringVar(&cfg.ProjectRoot, "project-root", "", "the project's root, where .turnkey/sync.toml is (required)")
	fs.BoolVar(&cfg.DryRun, "dry-run", false, "report what would change without writing")
	fs.BoolVar(&cfg.Verbose, "verbose", false, "print what sync does on stderr")
	fs.BoolVar(&cfg.Force, "force", false, "sync every rules.star, not only those whose sources changed")
	if err := fs.Parse(args); err != nil {
		return 2
	}
	if cfg.ProjectRoot == "" || fs.NArg() > 1 {
		fs.Usage()
		return 2
	}
	dir := cfg.ProjectRoot
	if fs.NArg() == 1 {
		dir = fs.Arg(0)
	}

	report := syncRules(cfg, dir)
	enc := json.NewEncoder(stdout)
	if err := enc.Encode(report); err != nil {
		fmt.Fprintf(stderr, "rules-sync: writing the report: %v\n", err)
		return 1
	}
	if report.Error != nil {
		return 1
	}
	return 0
}

// syncRules syncs the rules.star files under dir, and reports what it did.
func syncRules(cfg rulessync.Config, dir string) rulesreport.Report {
	syncer, err := rulessync.NewSyncer(cfg)
	if err != nil {
		return rulesreport.Report{Error: &rulesreport.Error{Stage: rulesreport.StageSetup, Message: err.Error()}}
	}
	results, err := syncer.SyncDirectory(dir)
	if err != nil {
		return rulesreport.Report{Error: &rulesreport.Error{Stage: rulesreport.StageSync, Message: err.Error()}}
	}
	report := rulesreport.Report{Results: make([]rulesreport.Result, 0, len(results))}
	for _, r := range results {
		report.Results = append(report.Results, toResult(r))
	}
	return report
}

// toResult is the report's record of a rules.star file's sync result.
func toResult(r rulessync.SyncResult) rulesreport.Result {
	result := rulesreport.Result{
		Path:     r.Path,
		Updated:  r.Updated,
		Skipped:  r.Skipped,
		OptedOut: r.OptedOut,
		Errors:   r.Errors,
	}
	for _, c := range r.Changes {
		result.Changes = append(result.Changes, rulesreport.TargetChange{
			Target:    c.Target,
			Attribute: c.Attribute,
			Added:     c.Added,
			Removed:   c.Removed,
			Kept:      c.Kept,
			Unmapped:  c.Unmapped,
		})
	}
	for _, u := range r.Unreadable {
		result.Unreadable = append(result.Unreadable, rulesreport.UnreadableTarget{
			Target:    u.Target,
			Attribute: u.Attribute,
		})
	}
	return result
}
