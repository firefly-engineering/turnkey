// tw is the turnkey wrapper for native language tools.
//
// It transparently wraps tools like go, cargo, and uv, detecting when they
// modify dependency files and automatically triggering sync operations.
//
// Usage:
//
//	tw go get github.com/foo/bar    # runs go get, syncs if go.mod changed
//	tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
//	tw uv add requests              # runs uv add, syncs if pyproject.toml changed
//
// Configuration is read from .turnkey/sync.toml:
//
//	[[wrappers]]
//	name = "go"
//	command = "go"
//	mutating_subcommands = ["get", "mod"]
//	watch_files = ["go.mod", "go.sum"]
//	deps_rule = "go"
package main

import (
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"syscall"

	"github.com/firefly-engineering/turnkey/src/go/pkg/snapshot"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncer"
)

var (
	verbose bool
	noSync  bool
)

// defaultWrapperRules provides sensible defaults for common tools.
// These are used when no [[wrappers]] section exists in sync.toml.
var defaultWrapperRules = map[string]*syncconfig.WrapperRule{
	"go": {
		Name:                "go",
		Command:             "go",
		MutatingSubcommands: []string{"get", "mod"},
		WatchFiles:          []string{"go.mod", "go.sum"},
		DepsRule:            "go",
		PostCommands:        []string{"go mod tidy"},
	},
	"cargo": {
		Name:                "cargo",
		Command:             "cargo",
		MutatingSubcommands: []string{"add", "remove", "update"},
		WatchFiles:          []string{"Cargo.toml", "Cargo.lock"},
		DepsRule:            "rust",
	},
	"uv": {
		Name:                "uv",
		Command:             "uv",
		MutatingSubcommands: []string{"add", "remove", "lock", "sync"},
		WatchFiles:          []string{"pyproject.toml", "uv.lock"},
		DepsRule:            "python",
	},
}

func main() {
	args := os.Args[1:]

	// Parse tw-specific flags
	args = parseFlags(args)

	// Need at least a tool name
	if len(args) == 0 {
		printHelp()
		os.Exit(0)
	}

	toolName := args[0]
	toolArgs := args[1:]

	// Find project root
	root, err := findProjectRoot()
	if err != nil {
		// No project root found - just pass through
		if verbose {
			fmt.Fprintf(os.Stderr, "tw: no project root found, passing through\n")
		}
		runToolAndExit(toolName, toolArgs)
	}

	// Load configuration
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tw: failed to load config: %v\n", err)
		runToolAndExit(toolName, toolArgs)
	}

	// Find wrapper rule for this tool (config takes precedence over defaults)
	rule := cfg.FindWrapper(toolName)
	if rule == nil {
		// Try default rules
		rule = defaultWrapperRules[toolName]
	}
	if rule == nil {
		// No wrapper configured for this tool - just pass through
		if verbose {
			fmt.Fprintf(os.Stderr, "tw: no wrapper rule for %q, passing through\n", toolName)
		}
		runToolAndExit(toolName, toolArgs)
	}
	if verbose && cfg.FindWrapper(toolName) == nil {
		fmt.Fprintf(os.Stderr, "tw: using default wrapper rule for %q\n", toolName)
	}

	// Determine if this is a mutating subcommand
	subcommand := ""
	if len(toolArgs) > 0 {
		subcommand = toolArgs[0]
	}
	isMutating := rule.IsMutatingSubcommand(subcommand)

	if !isMutating || noSync {
		// Not a mutating command or sync disabled - just run
		if verbose && noSync {
			fmt.Fprintf(os.Stderr, "tw: sync disabled, passing through\n")
		} else if verbose {
			fmt.Fprintf(os.Stderr, "tw: %q is not a mutating subcommand, passing through\n", subcommand)
		}
		runToolAndExit(toolName, toolArgs)
	}

	// Mutating command - capture before state
	if verbose {
		fmt.Fprintf(os.Stderr, "tw: capturing state of %v\n", rule.WatchFiles)
	}
	beforeSnap, err := snapshot.Capture(root, rule.WatchFiles)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tw: failed to capture before state: %v\n", err)
		runToolAndExit(toolName, toolArgs)
	}

	// Run the tool
	exitCode := runTool(toolName, toolArgs)

	// Capture after state
	afterSnap, err := snapshot.Capture(root, rule.WatchFiles)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tw: failed to capture after state: %v\n", err)
		os.Exit(exitCode)
	}

	// Check for changes
	if snapshot.Changed(beforeSnap, afterSnap) {
		if verbose {
			fmt.Fprintf(os.Stderr, "tw: detected changes in %v\n", rule.WatchFiles)
		}

		// Run post-commands (e.g., "go mod tidy" after "go get")
		for _, postCmd := range rule.PostCommands {
			if verbose {
				fmt.Fprintf(os.Stderr, "tw: running post-command: %s\n", postCmd)
			}
			postExitCode := runPostCommand(postCmd, root)
			if postExitCode != 0 {
				fmt.Fprintf(os.Stderr, "tw: post-command %q failed with exit code %d\n", postCmd, postExitCode)
				// Continue with sync anyway - the post-command failure shouldn't block sync
			}
		}

		// Run sync
		if verbose {
			fmt.Fprintf(os.Stderr, "tw: running sync\n")
		}
		syncExitCode := runSyncForRule(cfg, rule, root)
		if syncExitCode != 0 {
			fmt.Fprintf(os.Stderr, "tw: sync failed with exit code %d\n", syncExitCode)
		}
	} else if verbose {
		fmt.Fprintf(os.Stderr, "tw: no changes detected\n")
	}

	os.Exit(exitCode)
}

// parseFlags extracts tw-specific flags from the beginning of args.
func parseFlags(args []string) []string {
	for len(args) > 0 {
		switch args[0] {
		case "--verbose", "-v":
			verbose = true
			args = args[1:]
		case "--no-sync":
			noSync = true
			args = args[1:]
		case "--help", "-h":
			printHelp()
			os.Exit(0)
		default:
			return args
		}
	}
	return args
}

// runTool executes the tool with the given arguments and returns the exit code.
// It forwards signals to the child process.
func runTool(name string, args []string) int {
	toolPath := findRealTool(name)
	if toolPath == "" {
		fmt.Fprintf(os.Stderr, "tw: %s not found in PATH\n", name)
		return 1
	}

	cmd := exec.Command(toolPath, args...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	// Forward signals to child
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
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode()
		}
		return 1
	}
	return 0
}

// runToolAndExit executes the tool and exits with its exit code.
// This is used when no sync is needed - we just pass through.
func runToolAndExit(name string, args []string) {
	os.Exit(runTool(name, args))
}

// runPostCommand runs a post-command string (e.g., "go mod tidy").
func runPostCommand(cmdStr, dir string) int {
	parts := strings.Fields(cmdStr)
	if len(parts) == 0 {
		return 0
	}

	toolPath := findRealTool(parts[0])
	if toolPath == "" {
		fmt.Fprintf(os.Stderr, "tw: post-command tool %q not found\n", parts[0])
		return 1
	}

	cmd := exec.Command(toolPath, parts[1:]...)
	cmd.Dir = dir
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	err := cmd.Run()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode()
		}
		return 1
	}
	return 0
}

// runSyncForRule runs the sync operation for the specified deps rule.
func runSyncForRule(cfg *syncconfig.Config, wrapper *syncconfig.WrapperRule, root string) int {
	depsRule := cfg.FindDepsRule(wrapper.DepsRule)
	if depsRule == nil {
		fmt.Fprintf(os.Stderr, "tw: deps rule %q not found\n", wrapper.DepsRule)
		return 1
	}

	s := syncer.New(cfg, root)
	s.Verbose = verbose
	s.Output = os.Stderr

	err := s.SyncRule(*depsRule)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tw: sync error: %v\n", err)
		return 1
	}

	return 0
}

// findRealTool finds the real tool binary, avoiding wrapper recursion.
// It first checks TURNKEY_REAL_<TOOL> env var (set by shell wrappers),
// then falls back to PATH lookup.
func findRealTool(name string) string {
	// Check for explicit path from shell wrapper (avoids recursion)
	envVar := "TURNKEY_REAL_" + strings.ToUpper(name)
	if path := os.Getenv(envVar); path != "" {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}

	// Fall back to PATH lookup
	path, err := exec.LookPath(name)
	if err != nil {
		return ""
	}
	return path
}

// findProjectRoot walks up from cwd to find the project root.
// The project root is identified by the presence of .buckconfig or .turnkey/sync.toml.
func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("failed to get working directory: %w", err)
	}

	for {
		// Check for .buckconfig
		if _, err := os.Stat(dir + "/.buckconfig"); err == nil {
			return dir, nil
		}
		// Check for .turnkey/sync.toml
		if _, err := os.Stat(dir + "/.turnkey/sync.toml"); err == nil {
			return dir, nil
		}

		parent := dir[:max(0, len(dir)-len("/"+dir[strings.LastIndex(dir, "/")+1:]))]
		if parent == dir || parent == "" {
			break
		}
		dir = parent
	}

	return "", fmt.Errorf("no project root found (looking for .buckconfig or .turnkey/sync.toml)")
}

func printHelp() {
	fmt.Print(`tw - turnkey wrapper for native language tools

Usage: tw [tw-flags] <tool> [tool-args...]

tw transparently wraps native language tools (go, cargo, uv), detecting when
they modify dependency files and automatically triggering sync operations.

tw-specific flags (must come before tool name):
  --verbose    Show what tw is doing
  -v           Same as --verbose
  --no-sync    Disable automatic sync after tool runs
  --help       Show this help
  -h           Same as --help

Examples:
  tw go get github.com/foo/bar    # runs go get, then go mod tidy, then sync
  tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
  tw uv add requests              # runs uv add, syncs if pyproject.toml changed
  tw go build ./...               # just runs go build (not a mutating command)
  tw --no-sync go get foo         # runs go get without post-commands or sync

Default behavior for Go:
  After 'go get' or 'go mod' commands, tw automatically runs 'go mod tidy'
  to ensure direct/indirect dependencies are correctly classified before
  syncing go-deps.toml.

Configuration:
  tw reads wrapper rules from .turnkey/sync.toml:

  [[wrappers]]
  name = "go"
  command = "go"
  mutating_subcommands = ["get", "mod"]
  watch_files = ["go.mod", "go.sum"]
  deps_rule = "go"
  post_commands = ["go mod tidy"]  # run after main command, before sync

Environment:
  TURNKEY_NO_WRAP=1   Bypass tw wrapper entirely (use real tool)
`)
}
