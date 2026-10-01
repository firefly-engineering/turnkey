//! `tk sync`, `tk check`, and the deps sync before a buck2 command: the
//! deps rules of .turnkey/sync.toml, through project-sync's syncer

use std::io::Write;

use project_sync::syncer::Syncer;

use crate::Env;
use crate::flags::Flags;

/// Syncs (regenerate) or checks the deps rules named in only, or every
/// rule when none is. explicit is whether the user asked for it (tk sync,
/// tk check) rather than tk running it before a buck2 command, which says
/// nothing when there's nothing to do. Returns the exit code.
pub fn run_deps(
    env: &Env,
    flags: &Flags,
    regenerate: bool,
    only: Vec<String>,
    explicit: bool,
) -> i32 {
    let mut stderr = std::io::stderr();
    let root = match env.project_root() {
        Ok(root) => root,
        Err(e) => {
            let _ = writeln!(stderr, "tk: {e}");
            return 1;
        }
    };
    let mut s = match Syncer::load(&root, env.launcher.clone()) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "tk: {e:#}");
            return 1;
        }
    };
    s.verbose = flags.verbose;
    s.quiet = flags.quiet;
    s.dry_run = flags.dry_run;
    s.output = Box::new(std::io::stderr());
    s.only = only;

    let (operation, outcome) = if regenerate {
        ("sync", s.sync_deps().map(|r| (r, false)))
    } else {
        ("check", s.check())
    };
    let (result, stale) = match outcome {
        Ok(outcome) => outcome,
        Err(e) => {
            let _ = writeln!(stderr, "tk: {operation} failed: {e:#}");
            return 1;
        }
    };
    if !result.errors.is_empty() {
        let _ = writeln!(
            stderr,
            "tk: {operation} completed with {} error(s):",
            result.errors.len()
        );
        for e in &result.errors {
            let _ = writeln!(stderr, "  - {e:#}");
        }
        return 1;
    }

    if regenerate && result.synced > 0 {
        let _ = writeln!(stderr, "tk: synced {} file(s)", result.synced);
    } else if stale {
        let _ = writeln!(stderr, "tk: some files are stale, run 'tk sync' to update");
        return 1;
    } else if !flags.quiet && (explicit || flags.verbose) {
        let _ = writeln!(stderr, "tk: all files up-to-date");
    }
    0
}
