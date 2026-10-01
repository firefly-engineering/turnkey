//! tk's help texts

/// `tk --help`, printed to stdout
pub const HELP: &str = r#"tk - turnkey CLI wrapper for buck2

Usage: tk [tk-flags] <subcommand> [buck2-args...]

tk automatically runs sync operations before buck2 commands that read
the build graph, ensuring generated files are up-to-date.

tk-specific flags (must come before subcommand):
  --no-sync         Skip dependency sync, run buck2 directly
  --no-rules-sync   Skip rules.star sync (still runs deps sync)
  --strict-rules    Fail if rules.star files would change (CI mode)
  --no-local        Skip local target overrides from .turnkey/local.toml
  --rerun           Run every test instead of reusing recorded results
                    (fresh passes are still recorded)
  --verbose         Show what tk is doing
  -v                Same as --verbose
  --quiet           Suppress non-error output
  -q                Same as --quiet
  --dry-run         Show what would be synced without doing it
  -n                Same as --dry-run
  --help            Show this help
  -h                Same as --help

tk-specific subcommands:
  sync [rule...]   Regenerate stale dependency files (every deps rule, or the named ones)
  check [rule...]  Check if dependency files are stale
  rules            Manage rules.star files (check, sync)
  compose          Edit external dependencies (edit, patch, reset, status)
  completion       Generate shell completion scripts (bash, zsh, fish)
  materialize      Bring deps cells in line with their cell indexes (the shell runs it)

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
  tk --rerun test //some:target       # re-run, ignoring recorded results
  tk sync                             # just run sync
  tk check                            # check staleness
  tk clean                            # clean (no sync needed)
  tk --dry-run sync                   # show what would be synced
  tk --isolation-dir=test build //foo # uses .turnkey-test isolation
"#;

/// `tk compose help`, printed to stderr (with a newline after it)
pub const COMPOSE_HELP: &str = r#"Usage: tk compose <command> [options]

Manage edits to external dependencies. Edited files are stored in .turnkey/edits/
and can be converted to patches for integration with Nix fixups.

Commands:
  status              Show edited files and generated patches
  edit <cell/path>    Copy a file from a cell to edits for modification
  patch [cell]        Generate patches from edited files
  reset [cell/path]   Revert edits (all, by cell, or specific file)

Examples:
  # Copy a file for editing
  tk compose edit godeps/vendor/github.com/spf13/cobra/command.go

  # Show what's being edited
  tk compose status

  # Generate patches from all edits
  tk compose patch

  # Generate patches for a specific cell
  tk compose patch godeps

  # Revert all edits
  tk compose reset

  # Revert edits for a specific cell
  tk compose reset godeps

  # Revert a specific file
  tk compose reset godeps/vendor/github.com/spf13/cobra/command.go

Options:
  --verbose, -v    Show detailed output
  --force, -f      Skip confirmation prompts (for reset)
  --patches, -p    Show patches in status output"#;
