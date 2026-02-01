// tk is the turnkey CLI wrapper for buck2.
//
// It automatically runs sync operations before buck2 commands that read
// the build graph, ensuring generated files are up-to-date.
//
// Usage:
//
//	tk build //some:target     # syncs first, then runs buck2 build
//	tk test //some:target      # syncs first, then runs buck2 test
//	tk clean                   # passes through directly (no sync)
//	tk --no-sync build ...     # skip sync, run buck2 directly
//
// The passThroughCommands list is configurable at the top of this file.
// All other commands (build, run, test, query, etc.) will sync first.
package main

import (
	"fmt"
	"os"
	"os/exec"
	"slices"
	"strings"
	"syscall"

	"github.com/firefly-engineering/turnkey/src/go/pkg/localconfig"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncer"
)

// passThroughCommands are buck2 subcommands that don't read the build
// graph and can be passed through directly without syncing.
// All other commands will have sync run before them.
var passThroughCommands = []string{
	"clean",
	"kill",
	"killall",
	"status",
	"log",
	"rage",
	"help",
	"docs",
	"init",
}

// Flags
var (
	noSync      bool
	noRulesSync bool
	strictRules bool
	noLocal     bool
	verbose     bool
	dryRun      bool
	quiet       bool
)

func main() {
	args := os.Args[1:]

	// Parse tk-specific flags (before the subcommand)
	args = parseFlags(args)

	// If no subcommand, show help
	if len(args) == 0 {
		printHelp()
		os.Exit(0)
	}

	subcommand := args[0]

	// Handle tk-specific subcommands
	switch subcommand {
	case "sync":
		exitCode := runSync()
		os.Exit(exitCode)
	case "check":
		exitCode := runCheck()
		os.Exit(exitCode)
	case "rules":
		exitCode := runRules(args[1:])
		os.Exit(exitCode)
	case "compose":
		exitCode := runCompose(args[1:])
		os.Exit(exitCode)
	case "completion":
		exitCode := runCompletion(args[1:])
		os.Exit(exitCode)
	}

	// Determine if this command needs sync first
	needsSync := shouldSync(subcommand)

	if needsSync && !noSync {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: syncing before %s...\n", subcommand)
		}
		if exitCode := runSync(); exitCode != 0 {
			os.Exit(exitCode)
		}
	}

	// Run rules sync if enabled (after deps sync, before buck2)
	if needsSync && !noRulesSync {
		if exitCode := runRulesAutoSync(); exitCode != 0 {
			os.Exit(exitCode)
		}
	}

	// Delegate to buck2
	delegateToBuck2(args)
}

// parseFlags extracts tk-specific flags from the beginning of args.
// Returns the remaining args after flags.
func parseFlags(args []string) []string {
	for len(args) > 0 {
		switch args[0] {
		case "--no-sync":
			noSync = true
			args = args[1:]
		case "--no-rules-sync":
			noRulesSync = true
			args = args[1:]
		case "--strict-rules":
			strictRules = true
			args = args[1:]
		case "--no-local":
			noLocal = true
			args = args[1:]
		case "--verbose", "-v":
			verbose = true
			args = args[1:]
		case "--dry-run", "-n":
			dryRun = true
			args = args[1:]
		case "--quiet", "-q":
			quiet = true
			args = args[1:]
		case "--help", "-h":
			printHelp()
			os.Exit(0)
		default:
			// Not a tk flag, done parsing
			return args
		}
	}
	return args
}

// shouldSync returns true if the subcommand should have sync run first.
// Unknown commands default to requiring sync (safe default).
func shouldSync(subcommand string) bool {
	// Explicit pass-through commands don't need sync
	if slices.Contains(passThroughCommands, subcommand) {
		return false
	}

	// Sync-first commands and unknown commands need sync
	// Unknown commands default to sync (safe default)
	return true
}

// runSync runs the turnkey sync operation.
// Returns exit code (0 for success, non-zero for failure).
func runSync() int {
	// Find project root (where .buckconfig is)
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		return 1
	}

	// Load configuration
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to load sync config: %v\n", err)
		return 1
	}

	// Validate configuration
	if err := cfg.Validate(); err != nil {
		fmt.Fprintf(os.Stderr, "tk: invalid sync config: %v\n", err)
		return 1
	}

	// Run sync
	s := syncer.New(cfg, root)
	s.Verbose = verbose
	s.Quiet = quiet
	s.DryRun = dryRun
	s.Output = os.Stderr

	result, err := s.SyncDeps()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: sync failed: %v\n", err)
		return 1
	}

	// Report results
	if len(result.Errors) > 0 {
		fmt.Fprintf(os.Stderr, "tk: sync completed with %d error(s):\n", len(result.Errors))
		for _, e := range result.Errors {
			fmt.Fprintf(os.Stderr, "  - %v\n", e)
		}
		return 1
	}

	if result.Synced > 0 {
		fmt.Fprintf(os.Stderr, "tk: synced %d file(s)\n", result.Synced)
	} else if verbose && !quiet {
		fmt.Fprintln(os.Stderr, "tk: nothing to sync, all files up-to-date")
	}

	return 0
}

// runCheck checks if any files are stale without regenerating them.
// Returns exit code (0 if all up-to-date, 1 if stale).
func runCheck() int {
	// Find project root
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		return 1
	}

	// Load configuration
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to load sync config: %v\n", err)
		return 1
	}

	// Run check
	s := syncer.New(cfg, root)
	s.Verbose = verbose
	s.Quiet = quiet
	s.Output = os.Stderr

	result, anyStale, err := s.Check()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: check failed: %v\n", err)
		return 1
	}

	// Report results
	if len(result.Errors) > 0 {
		fmt.Fprintf(os.Stderr, "tk: check completed with %d error(s):\n", len(result.Errors))
		for _, e := range result.Errors {
			fmt.Fprintf(os.Stderr, "  - %v\n", e)
		}
		return 1
	}

	if anyStale {
		fmt.Fprintln(os.Stderr, "tk: some files are stale, run 'tk sync' to update")
		return 1
	}

	if !quiet {
		fmt.Fprintln(os.Stderr, "tk: all files up-to-date")
	}
	return 0
}

// runCompletion generates shell completion scripts.
// It wraps buck2's completion output and adds tk-specific completions.
func runCompletion(args []string) int {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "Usage: tk completion <bash|zsh|fish>")
		return 1
	}

	shell := args[0]
	if shell != "bash" && shell != "zsh" && shell != "fish" {
		fmt.Fprintf(os.Stderr, "tk: unsupported shell: %s (use bash, zsh, or fish)\n", shell)
		return 1
	}

	// Get buck2's completion output
	cmd := exec.Command("buck2", "completion", shell)
	output, err := cmd.Output()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to get buck2 completion: %v\n", err)
		return 1
	}

	// Transform the completion script
	script := string(output)
	script = transformCompletion(script, shell)

	fmt.Print(script)
	return 0
}

// transformCompletion transforms buck2's completion script for tk.
func transformCompletion(script, shell string) string {
	switch shell {
	case "bash":
		return transformBashCompletion(script)
	case "zsh":
		return transformZshCompletion(script)
	case "fish":
		return transformFishCompletion(script)
	default:
		return script
	}
}

// transformBashCompletion transforms bash completion for tk.
func transformBashCompletion(script string) string {
	// Replace function name and references
	script = strings.ReplaceAll(script, "_buck2()", "_tk()")
	script = strings.ReplaceAll(script, "_buck2 ", "_tk ")
	script = strings.ReplaceAll(script, "complete -F _buck2", "complete -F _tk")
	script = strings.ReplaceAll(script, "buck2__", "tk__")
	script = strings.ReplaceAll(script, "\"buck2\"", "\"tk\"")
	script = strings.ReplaceAll(script, "buck2,", "tk,")
	script = strings.ReplaceAll(script, ",buck2)", ",tk)")

	// Add tk-specific subcommands to the case statement
	// Insert after "tk,completion)" case
	script = strings.Replace(script,
		"tk,completion)\n                cmd=\"tk__completion\"",
		"tk,check)\n                cmd=\"tk__check\"\n                ;;\n            tk,completion)\n                cmd=\"tk__completion\"",
		1)

	// Add sync command
	script = strings.Replace(script,
		"tk,check)\n                cmd=\"tk__check\"",
		"tk,check)\n                cmd=\"tk__check\"\n                ;;\n            tk,sync)\n                cmd=\"tk__sync\"",
		1)

	// Add tk-specific flags and subcommands to the main opts
	// Find the main opts line and add tk-specific items
	script = strings.Replace(script,
		"opts=\"-v -h -V --isolation-dir",
		"opts=\"--no-sync --dry-run -v -h -V --isolation-dir",
		1)

	// Add sync and check to subcommand list (after "completion")
	script = strings.Replace(script,
		"completion docs",
		"completion sync check docs",
		1)

	// Add completion handlers for tk-specific subcommands
	tkCompletions := `
        tk__check)
            opts="-v -h --verbose --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            ;;
        tk__sync)
            opts="-v -n -h --verbose --dry-run --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            ;;
`
	// Insert before the final esac in the main case statement
	// Find a good insertion point - after tk__completion case
	if idx := strings.Index(script, "tk__completion)"); idx != -1 {
		// Find the end of the tk__completion case (next case or esac)
		endIdx := strings.Index(script[idx:], "\n        tk__")
		if endIdx == -1 {
			endIdx = strings.Index(script[idx:], "\n    esac")
		}
		if endIdx != -1 {
			insertPoint := idx + endIdx
			script = script[:insertPoint] + tkCompletions + script[insertPoint:]
		}
	}

	// Update the header comment
	script = strings.Replace(script,
		"# @generated by `buck2 completion bash`",
		"# @generated by `tk completion bash` (based on buck2 completion)",
		1)

	return script
}

// transformZshCompletion transforms zsh completion for tk.
func transformZshCompletion(script string) string {
	// Replace function names and references
	script = strings.ReplaceAll(script, "_buck2", "_tk")
	script = strings.ReplaceAll(script, "#compdef buck2", "#compdef tk")
	script = strings.ReplaceAll(script, "'buck2'", "'tk'")
	script = strings.ReplaceAll(script, "\"buck2\"", "\"tk\"")
	script = strings.ReplaceAll(script, "buck2:", "tk:")

	// Add tk-specific flags to global options
	script = strings.Replace(script,
		"'--isolation-dir",
		"'--no-sync[Skip sync, run buck2 directly]' '--dry-run[Show what would be synced]' '--isolation-dir",
		1)

	// Add tk subcommands
	script = strings.Replace(script,
		"'completion:Print completion configuration for shell'",
		"'completion:Print completion configuration for shell' 'sync:Synchronize generated files' 'check:Check if files are stale'",
		1)

	// Update the header comment
	script = strings.Replace(script,
		"# @generated by `buck2 completion zsh`",
		"# @generated by `tk completion zsh` (based on buck2 completion)",
		1)

	return script
}

// transformFishCompletion transforms fish completion for tk.
func transformFishCompletion(script string) string {
	// Replace command name
	script = strings.ReplaceAll(script, "complete -c buck2", "complete -c tk")
	script = strings.ReplaceAll(script, "__fish_buck2", "__fish_tk")
	script = strings.ReplaceAll(script, "'buck2'", "'tk'")
	script = strings.ReplaceAll(script, "\"buck2\"", "\"tk\"")

	// Add tk-specific flags
	tkFlags := `
# tk-specific flags
complete -c tk -n "__fish_tk_needs_command" -l no-sync -d 'Skip sync, run buck2 directly'
complete -c tk -n "__fish_tk_needs_command" -s n -l dry-run -d 'Show what would be synced'

# tk-specific subcommands
complete -c tk -n "__fish_tk_needs_command" -a sync -d 'Synchronize generated files'
complete -c tk -n "__fish_tk_needs_command" -a check -d 'Check if files are stale'

# sync subcommand options
complete -c tk -n "__fish_seen_subcommand_from sync" -s v -l verbose -d 'Show verbose output'
complete -c tk -n "__fish_seen_subcommand_from sync" -s n -l dry-run -d 'Show what would be synced'

# check subcommand options
complete -c tk -n "__fish_seen_subcommand_from check" -s v -l verbose -d 'Show verbose output'
`
	script += tkFlags

	// Update the header comment
	script = strings.Replace(script,
		"# @generated by `buck2 completion fish`",
		"# @generated by `tk completion fish` (based on buck2 completion)",
		1)

	return script
}

// findProjectRoot walks up from cwd to find the project root.
// The project root is identified by the presence of .buckconfig.
func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("failed to get working directory: %w", err)
	}

	for {
		if _, err := os.Stat(dir + "/.buckconfig"); err == nil {
			return dir, nil
		}

		parent := dir[:max(0, len(dir)-len("/"+dir[strings.LastIndex(dir, "/")+1:]))]
		if parent == dir || parent == "" {
			break
		}
		dir = parent
	}

	// Fallback to current directory
	cwd, _ := os.Getwd()
	return cwd, nil
}

// delegateToBuck2 executes buck2 with the given arguments.
// It uses exec to replace the current process with buck2.
func delegateToBuck2(args []string) {
	buck2Path, err := exec.LookPath("buck2")
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: buck2 not found in PATH: %v\n", err)
		os.Exit(1)
	}

	// Transform --isolation-dir to use .turnkey prefix
	args = transformIsolationDir(args)

	// Apply local target overrides if enabled
	if !noLocal {
		args = applyLocalOverrides(args)
	}

	if verbose {
		fmt.Fprintf(os.Stderr, "tk: executing buck2 %v\n", args)
	}

	// Use syscall.Exec to replace this process with buck2
	// This ensures signals, stdio, etc. work correctly
	err = syscall.Exec(buck2Path, append([]string{"buck2"}, args...), os.Environ())
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to exec buck2: %v\n", err)
		os.Exit(1)
	}
}

// applyLocalOverrides loads local.toml and injects target-specific args.
// It looks for the first target in args (starting with //) and checks
// if there's an override for it in the config.
func applyLocalOverrides(args []string) []string {
	if len(args) == 0 {
		return args
	}

	// Find project root to load config
	root, err := findProjectRoot()
	if err != nil {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: could not find project root for local config: %v\n", err)
		}
		return args
	}

	// Load local config
	cfg, err := localconfig.LoadDefaultFrom(root)
	if err != nil {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: could not load local config: %v\n", err)
		}
		return args
	}

	if !cfg.HasOverrides() {
		return args
	}

	// Get the subcommand (first arg)
	subcommand := args[0]

	// Only apply to run, build, test
	if subcommand != "run" && subcommand != "build" && subcommand != "test" {
		return args
	}

	// Find the target (first arg starting with //)
	target := ""
	for _, arg := range args[1:] {
		if strings.HasPrefix(arg, "//") {
			target = arg
			break
		}
		// Stop at -- separator
		if arg == "--" {
			break
		}
	}

	if target == "" {
		return args
	}

	// Look up override
	override := cfg.GetOverride(subcommand, target)
	if override == nil || len(override.Args) == 0 {
		return args
	}

	if verbose {
		fmt.Fprintf(os.Stderr, "tk: applying local override for %s %s: %v\n", subcommand, target, override.Args)
	}

	// Inject args after existing -- or add new --
	return injectArgsAfterSeparator(args, override.Args)
}

// injectArgsAfterSeparator appends args after the -- separator.
// If no -- exists, it adds one and then the args.
func injectArgsAfterSeparator(args []string, toInject []string) []string {
	// Find existing -- separator
	separatorIdx := -1
	for i, arg := range args {
		if arg == "--" {
			separatorIdx = i
			break
		}
	}

	if separatorIdx == -1 {
		// No separator, add -- and args at the end
		result := make([]string, 0, len(args)+1+len(toInject))
		result = append(result, args...)
		result = append(result, "--")
		result = append(result, toInject...)
		return result
	}

	// Insert args after --
	result := make([]string, 0, len(args)+len(toInject))
	result = append(result, args[:separatorIdx+1]...)
	result = append(result, toInject...)
	result = append(result, args[separatorIdx+1:]...)
	return result
}

// transformIsolationDir transforms --isolation-dir values to use .turnkey prefix.
// This ensures all isolation directories are hidden from Go/Cargo/pytest.
//
// Transformation rules:
//   - --isolation-dir=foo     -> --isolation-dir=.turnkey-foo
//   - --isolation-dir=.custom -> --isolation-dir=.custom (already dotted, pass through)
//   - No --isolation-dir      -> unchanged (uses .turnkey from buckconfig)
func transformIsolationDir(args []string) []string {
	result := make([]string, 0, len(args))

	for i := 0; i < len(args); i++ {
		arg := args[i]

		// Handle --isolation-dir=value format
		if strings.HasPrefix(arg, "--isolation-dir=") {
			value := strings.TrimPrefix(arg, "--isolation-dir=")
			transformed := transformIsolationDirValue(value)
			result = append(result, "--isolation-dir="+transformed)
			continue
		}

		// Handle --isolation-dir value format (two separate args)
		if arg == "--isolation-dir" && i+1 < len(args) {
			value := args[i+1]
			transformed := transformIsolationDirValue(value)
			result = append(result, "--isolation-dir", transformed)
			i++ // Skip the next arg since we consumed it
			continue
		}

		result = append(result, arg)
	}

	return result
}

// transformIsolationDirValue transforms an isolation dir value.
// If it already starts with ".", it's passed through unchanged.
// Otherwise, it's prefixed with ".turnkey-".
func transformIsolationDirValue(value string) string {
	if strings.HasPrefix(value, ".") {
		// Already starts with dot, pass through
		return value
	}
	// Prefix with .turnkey-
	return ".turnkey-" + value
}

func printHelp() {
	fmt.Print(`tk - turnkey CLI wrapper for buck2

Usage: tk [tk-flags] <subcommand> [buck2-args...]

tk automatically runs sync operations before buck2 commands that read
the build graph, ensuring generated files are up-to-date.

tk-specific flags (must come before subcommand):
  --no-sync         Skip dependency sync, run buck2 directly
  --no-rules-sync   Skip rules.star sync (still runs deps sync)
  --strict-rules    Fail if rules.star files would change (CI mode)
  --no-local        Skip local target overrides from .turnkey/local.toml
  --verbose         Show what tk is doing
  -v                Same as --verbose
  --quiet           Suppress non-error output
  -q                Same as --quiet
  --dry-run         Show what would be synced without doing it
  -n                Same as --dry-run
  --help            Show this help
  -h                Same as --help

tk-specific subcommands:
  sync         Run dependency sync manually
  check        Check if dependency files are stale
  rules        Manage rules.star files (check, sync)
  compose      Edit external dependencies (edit, patch, reset, status)
  completion   Generate shell completion scripts (bash, zsh, fish)

All other subcommands are delegated to buck2.

Commands that sync first (read build graph):
  build, run, test, query, cquery, uquery, targets, audit, bxl

Commands that pass through directly (no sync):
  clean, kill, killall, status, log, rage, help, docs, init

Unknown commands default to syncing first (safe default).

Isolation Directory:
  tk transforms --isolation-dir to use .turnkey prefix, ensuring build
  artifacts are hidden from Go/Cargo/pytest (which ignore dot directories).

  --isolation-dir=foo     -> buck2 --isolation-dir=.turnkey-foo
  --isolation-dir=.custom -> buck2 --isolation-dir=.custom (unchanged)
  (no flag)               -> uses .turnkey from buckconfig

Local Target Overrides:
  tk reads .turnkey/local.toml for per-developer target overrides.
  This file is not committed to git, allowing local customization.

  Example .turnkey/local.toml:
    [run."//docs/user-manual"]
    args = ["-n", "192.168.1.100"]

    [test."//src/..."]
    args = ["--verbose"]

  The args are injected after -- when running that target.
  Use --no-local to skip applying local overrides.

Configuration:
  tk reads .turnkey/sync.toml for staleness rules.
  tk reads .turnkey/local.toml for local target overrides.

Examples:
  tk build //some:target              # sync then build
  tk test //some:target               # sync then test
  tk --no-sync build //some:target    # skip sync
  tk --no-local run //target          # skip local overrides
  tk sync                             # just run sync
  tk check                            # check staleness
  tk clean                            # clean (no sync needed)
  tk --dry-run sync                   # show what would be synced
  tk --isolation-dir=test build //foo # uses .turnkey-test isolation
`)
}
