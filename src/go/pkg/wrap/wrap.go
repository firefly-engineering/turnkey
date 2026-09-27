// Package wrap runs a native tool (go, cargo, uv) for tw, and syncs the
// project's deps when the tool changes the files its wrapper rule watches.
//
// A Wrapper runs the tool, and when the tool was run with one of its
// wrapper rule's mutating subcommands and changed a watched file, runs the
// rule's post-commands and syncs the rule's deps rule. Either way it
// returns the tool's exit code. Running a tool and syncing are adapters
// (Exec, Sync), so the module is tested with a fake tool.
//
// A change is a change of content: the watched files are hashed before and
// after the tool runs (package snapshot), where tk sync compares mtimes
// (package staleness). The two answer different questions. tk sync asks,
// with no memory of earlier runs, whether a target is older than its
// sources, which a stat of each file answers before every build. tw asks
// whether this one run changed the files, and has their state from before
// it: comparing content keeps a tool that rewrites a file without changing
// it from setting off a regeneration, which can mean prefetching every
// dependency, and doesn't depend on the filesystem's mtime resolution.
package wrap

import (
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"syscall"

	"github.com/firefly-engineering/turnkey/src/go/pkg/snapshot"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncer"
)

// Exec runs tool with args in dir (the working directory when empty),
// connected to the caller's stdio, and returns its exit code.
type Exec func(dir, tool string, args []string) int

// Sync regenerates the deps rule named rule, then every rule left stale
// by it.
type Sync func(rule string) error

// Wrapper runs tools for one project.
type Wrapper struct {
	// Root is the project root, where watched files are read and
	// post-commands run.
	Root string
	// Config holds the wrapper rules; a tool without one is passed
	// through.
	Config *syncconfig.Config
	// Exec runs the tool and the post-commands.
	Exec Exec
	// Sync syncs a wrapper rule's deps rule.
	Sync Sync
	// NoSync passes every tool through.
	NoSync bool
	// Verbose says what the wrapper does.
	Verbose bool
	// Log is where the wrapper's messages go.
	Log io.Writer
}

// Open returns the Wrapper for the project dir is in, syncing with the
// project's syncer. Outside a project, or when its sync.toml doesn't load,
// the Wrapper passes every tool through.
func Open(dir string, run Exec, log io.Writer) *Wrapper {
	w := &Wrapper{Config: &syncconfig.Config{}, Exec: run, Log: log}
	root, ok := syncconfig.FindRoot(dir)
	if !ok {
		w.Sync = func(string) error { return errors.New("no project root") }
		return w
	}
	w.Root = root
	s, err := syncer.Load(root)
	if err != nil {
		w.logf("tw: %v\n", err)
		w.Sync = func(string) error { return err }
		return w
	}
	w.Config = s.Config
	w.Sync = func(rule string) error { return syncRule(s, rule, w.Verbose, w.Log) }
	return w
}

// Run runs tool with args and returns its exit code, syncing when a
// mutating subcommand changed a watched file.
func (w *Wrapper) Run(tool string, args []string) int {
	rule := w.Config.FindWrapper(tool)
	switch {
	case w.Root == "":
		w.verbosef("tw: no project root found, passing through\n")
		return w.Exec("", tool, args)
	case rule == nil:
		w.verbosef("tw: no wrapper rule for %q, passing through\n", tool)
		return w.Exec("", tool, args)
	case w.NoSync:
		w.verbosef("tw: sync disabled, passing through\n")
		return w.Exec("", tool, args)
	}
	subcommand := ""
	if len(args) > 0 {
		subcommand = args[0]
	}
	if !rule.IsMutatingSubcommand(subcommand) {
		w.verbosef("tw: %q is not a mutating subcommand, passing through\n", subcommand)
		return w.Exec("", tool, args)
	}

	w.verbosef("tw: capturing state of %v\n", rule.WatchFiles)
	before, err := snapshot.Capture(w.Root, rule.WatchFiles)
	if err != nil {
		w.logf("tw: failed to capture before state: %v\n", err)
		return w.Exec("", tool, args)
	}
	exitCode := w.Exec("", tool, args)
	after, err := snapshot.Capture(w.Root, rule.WatchFiles)
	if err != nil {
		w.logf("tw: failed to capture after state: %v\n", err)
		return exitCode
	}
	if !snapshot.Changed(before, after) {
		w.verbosef("tw: no changes detected\n")
		return exitCode
	}

	w.verbosef("tw: detected changes in %v\n", rule.WatchFiles)
	// Post-commands (go mod tidy after go get) settle the files before
	// they are synced; one failing doesn't stop the sync.
	for _, postCmd := range rule.PostCommands {
		parts := strings.Fields(postCmd)
		if len(parts) == 0 {
			continue
		}
		w.verbosef("tw: running post-command: %s\n", postCmd)
		if code := w.Exec(w.Root, parts[0], parts[1:]); code != 0 {
			w.logf("tw: post-command %q failed with exit code %d\n", postCmd, code)
		}
	}
	w.verbosef("tw: running sync\n")
	if err := w.Sync(rule.DepsRule); err != nil {
		w.logf("tw: sync failed: %v\n", err)
	}
	return exitCode
}

// syncRule regenerates the deps rule named name, then every rule left
// stale by it: a rule whose source is that rule's target (python-deps.toml
// from pylock.toml) comes after it in sync.toml.
func syncRule(s *syncer.Syncer, name string, verbose bool, log io.Writer) error {
	rule := s.Config.FindDepsRule(name)
	if rule == nil {
		return fmt.Errorf("deps rule %q not found", name)
	}
	s.Verbose = verbose
	s.Output = log
	if err := s.SyncRule(*rule); err != nil {
		return err
	}
	s.Quiet = !verbose
	result, err := s.SyncDeps()
	if err != nil {
		return err
	}
	return errors.Join(result.Errors...)
}

func (w *Wrapper) logf(format string, args ...any) {
	if w.Log != nil {
		_, _ = fmt.Fprintf(w.Log, format, args...)
	}
}

func (w *Wrapper) verbosef(format string, args ...any) {
	if w.Verbose {
		w.logf(format, args...)
	}
}

// RealExec runs the real tool: the one a shell wrapper names in
// TURNKEY_REAL_<TOOL>, so tw doesn't run the wrapper again, or else the
// one on PATH. It forwards SIGINT, SIGTERM and SIGHUP to the tool.
func RealExec(dir, tool string, args []string) int {
	path := findRealTool(tool)
	if path == "" {
		fmt.Fprintf(os.Stderr, "tw: %s not found in PATH\n", tool)
		return 1
	}

	cmd := exec.Command(path, args...)
	cmd.Dir = dir
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM, syscall.SIGHUP)
	go func() {
		for sig := range sigChan {
			if cmd.Process != nil {
				_ = cmd.Process.Signal(sig)
			}
		}
	}()

	err := cmd.Run()
	signal.Stop(sigChan)
	close(sigChan)

	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return exitErr.ExitCode()
		}
		return 1
	}
	return 0
}

// findRealTool returns the path of the real tool, or "" when there is none.
func findRealTool(name string) string {
	if path := os.Getenv("TURNKEY_REAL_" + strings.ToUpper(name)); path != "" {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}
	path, err := exec.LookPath(name)
	if err != nil {
		return ""
	}
	return path
}
