//! `tk rules` and the rules sync before a buck2 command: the rules.star
//! files' deps kept in step with their sources, through the rules-syncer
//! library
//!
//! The Go tk ran rules sync as a separate binary and read back its JSON
//! report (#203); here the report comes straight from the library.

use std::io::Write;
use std::path::Path;

use gostd::filepath;
use project_sync::config::Config as SyncConfig;
use project_sync::launch::Launcher;
use rules_syncer::report::{Error, Report, Result as FileResult, Stage, TargetChange};
use rules_syncer::sync::{Config, Syncer};

use crate::flags::Flags;

/// What tk asks rules sync to do: the inputs of the syncer's config, and
/// the directory to sync
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    pub project_root: String,
    pub dir: String,
    pub dry_run: bool,
    pub verbose: bool,
    pub force: bool,
}

/// Runs rules sync and returns its report; an error that stopped sync is
/// the report's
pub type RulesSync<'a> = &'a mut dyn FnMut(Request) -> Report;

/// The rules sync tk runs: the rules-syncer library, starting the tools it
/// runs with launcher, relative paths against cwd
pub fn syncer(launcher: Launcher, cwd: String) -> impl FnMut(Request) -> Report {
    move |req: Request| {
        let failed = |stage, message: String| Report {
            results: None,
            error: Some(Error { stage, message }),
        };
        let syncer = match Syncer::new(Config {
            project_root: req.project_root,
            dry_run: req.dry_run,
            verbose: req.verbose,
            force: req.force,
            sync: None,
            launcher: launcher.clone(),
            cwd: cwd.clone(),
        }) {
            Ok(syncer) => syncer,
            Err(err) => return failed(Stage::Setup, format!("{err:#}")),
        };
        match syncer.sync_directory(&req.dir) {
            Ok(results) => Report {
                results: Some(results),
                error: None,
            },
            Err(err) => failed(Stage::Sync, format!("{err:#}")),
        }
    }
}

/// `tk rules [check|sync|help] ...`, with the project root found by
/// find_root. Returns the exit code.
pub fn run(
    args: &[String],
    find_root: &dyn Fn() -> std::result::Result<String, String>,
    flags: &mut Flags,
    sync: RulesSync,
    stderr: &mut dyn Write,
) -> i32 {
    let Some((subcmd, subargs)) = args.split_first() else {
        print_help(stderr);
        return 0;
    };
    match subcmd.as_str() {
        "check" | "sync" => {
            let root = match find_root() {
                Ok(root) => root,
                Err(e) => {
                    let _ = writeln!(stderr, "tk rules: {e}");
                    return 1;
                }
            };
            if subcmd == "check" {
                check(sync, &root, subargs, flags, stderr)
            } else {
                sync_rules(sync, &root, subargs, flags, stderr)
            }
        }
        "help" | "--help" | "-h" => {
            print_help(stderr);
            0
        }
        other => {
            let _ = writeln!(
                stderr,
                "tk rules: unknown subcommand {}",
                gostd::strconv::quote(other)
            );
            print_help(stderr);
            1
        }
    }
}

/// filepath.Rel(root, path), "" where Go's errs
fn rel(root: &str, path: &str) -> String {
    filepath::rel(Path::new(root), Path::new(path))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The directory a rules subcommand works on: root, or the path args name
/// under it
fn target_dir(root: &str, target: Option<&str>) -> String {
    match target {
        Some(t) => filepath::join(&[root, t]).to_string_lossy().into_owned(),
        None => root.to_string(),
    }
}

/// Go's `%v` of a []string: `[a b]`
fn go_list(items: &[String]) -> String {
    format!("[{}]", items.join(" "))
}

/// Writes the error that stopped rules sync, as tk rules does
fn report_error(e: &Error, op: &str, stderr: &mut dyn Write) {
    if e.stage == Stage::Setup {
        let _ = writeln!(stderr, "tk rules: {}", e.message);
    } else {
        let _ = writeln!(stderr, "tk rules: {op} failed: {}", e.message);
    }
}

/// `tk rules check`: checks every rules.star under root, or under the
/// directory args names. It ignores git status and file times, which only
/// tell what changed since the last commit: a stale rules.star that was
/// committed is still stale. --all and --force are accepted and change
/// nothing.
pub fn check(
    sync: RulesSync,
    root: &str,
    args: &[String],
    flags: &mut Flags,
    stderr: &mut dyn Write,
) -> i32 {
    let mut target = None;
    for arg in args {
        match arg.as_str() {
            "--all" | "-a" | "--force" | "-f" => {}
            "--verbose" | "-v" => flags.verbose = true,
            "--quiet" | "-q" => flags.quiet = true,
            // Assume it's a directory path
            a if !a.is_empty() && !a.starts_with('-') => target = Some(a),
            _ => {}
        }
    }
    let report = sync(Request {
        project_root: root.to_string(),
        dir: target_dir(root, target),
        dry_run: true,
        verbose: flags.verbose,
        force: true,
    });
    if let Some(e) = &report.error {
        report_error(e, "check", stderr);
        return 1;
    }
    let results = report.results.unwrap_or_default();
    let (verbose, quiet) = (flags.verbose, flags.quiet);

    let mut any_needs_update = false;
    let mut any_unreadable = false;
    let mut checked = 0;
    let mut skipped = 0;
    for result in &results {
        let rel_path = rel(root, &result.path);
        if result.skipped {
            skipped += 1;
            if verbose {
                let _ = writeln!(stderr, "SKIPPED: {rel_path} (up-to-date)");
            }
            continue;
        }
        checked += 1;
        if !quiet {
            print_kept_deps(&rel_path, &result.changes, stderr);
            print_skipped_targets(&rel_path, result, verbose, stderr);
        }
        if !result.unreadable.is_empty() {
            any_unreadable = true;
        }
        if result.updated {
            any_needs_update = true;
            let _ = writeln!(stderr, "NEEDS UPDATE: {rel_path}");
            if verbose {
                print_target_changes(&result.changes, "Would add", "Would remove", stderr);
            }
        } else if verbose {
            let _ = writeln!(stderr, "OK:    {rel_path}");
        }
        if verbose {
            for e in &result.errors {
                let _ = writeln!(stderr, "       Warning: {e}");
            }
        }
    }

    if any_needs_update {
        let _ = writeln!(
            stderr,
            "\ntk rules: some rules.star files need updates, run 'tk rules sync' to update"
        );
        return 1;
    }
    if any_unreadable {
        let _ = writeln!(
            stderr,
            "\ntk rules: some targets' deps can't be synced (see UNREADABLE above)"
        );
        return 1;
    }
    if !quiet {
        let _ = write!(
            stderr,
            "tk rules: all rules.star files up-to-date ({checked} checked"
        );
        if skipped > 0 {
            let _ = write!(stderr, ", {skipped} skipped");
        }
        let _ = writeln!(stderr, ")");
    }
    0
}

/// `tk rules sync`: updates the stale rules.star files under root, or
/// under the directory args names (all of them with --all)
pub fn sync_rules(
    sync: RulesSync,
    root: &str,
    args: &[String],
    flags: &mut Flags,
    stderr: &mut dyn Write,
) -> i32 {
    let mut target = None;
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--all" | "-a" | "--force" | "-f" => force = true,
            "--verbose" | "-v" => flags.verbose = true,
            "--quiet" | "-q" => flags.quiet = true,
            "--dry-run" | "-n" => flags.dry_run = true,
            a if !a.is_empty() && !a.starts_with('-') => target = Some(a),
            _ => {}
        }
    }
    let (verbose, quiet, dry_run) = (flags.verbose, flags.quiet, flags.dry_run);
    let report = sync(Request {
        project_root: root.to_string(),
        dir: target_dir(root, target),
        dry_run,
        verbose,
        force,
    });
    if let Some(e) = &report.error {
        report_error(e, "sync", stderr);
        return 1;
    }
    let results = report.results.unwrap_or_default();

    let mut updated = 0;
    let mut skipped = 0;
    for result in &results {
        let rel_path = rel(root, &result.path);
        // Errors are reported, but a file still updated isn't a failure
        if verbose {
            for e in &result.errors {
                let _ = writeln!(stderr, "WARNING: {rel_path}: {e}");
            }
        }
        if !quiet {
            print_kept_deps(&rel_path, &result.changes, stderr);
            print_skipped_targets(&rel_path, result, verbose, stderr);
        }
        if result.skipped {
            skipped += 1;
            if verbose {
                let _ = writeln!(stderr, "SKIPPED: {rel_path} (up-to-date)");
            }
        } else if result.updated {
            updated += 1;
            if dry_run {
                let _ = writeln!(stderr, "WOULD UPDATE: {rel_path}");
            } else {
                let _ = writeln!(stderr, "UPDATED: {rel_path}");
            }
            if verbose {
                print_target_changes(&result.changes, "Added", "Removed", stderr);
            }
        } else if verbose {
            let _ = writeln!(stderr, "OK: {rel_path} (no changes)");
        }
    }

    if !quiet {
        if dry_run {
            let _ = write!(stderr, "\ntk rules: would update {updated} file(s)");
        } else {
            let _ = write!(stderr, "\ntk rules: updated {updated} file(s)");
        }
        if skipped > 0 {
            let _ = write!(stderr, ", skipped {skipped} (up-to-date)");
        }
        let _ = writeln!(stderr);
    }
    0
}

/// Prints each changed target's added and removed deps
fn print_target_changes(
    changes: &[TargetChange],
    added_label: &str,
    removed_label: &str,
    stderr: &mut dyn Write,
) {
    for c in changes {
        if c.added.is_empty() && c.removed.is_empty() {
            continue;
        }
        if c.attribute.is_empty() {
            let _ = writeln!(stderr, "       :{}", c.target);
        } else {
            let _ = writeln!(stderr, "       :{} ({})", c.target, c.attribute);
        }
        if !c.added.is_empty() {
            let _ = writeln!(stderr, "         {added_label}: {}", go_list(&c.added));
        }
        if !c.removed.is_empty() {
            let _ = writeln!(stderr, "         {removed_label}: {}", go_list(&c.removed));
        }
    }
}

/// Reports, for each target of a rules.star file, the deps sync kept
/// instead of removing because the target has unmapped imports
fn print_kept_deps(rel_path: &str, changes: &[TargetChange], stderr: &mut dyn Write) {
    for c in changes.iter().filter(|c| !c.kept.is_empty()) {
        let _ = writeln!(
            stderr,
            "KEPT: {rel_path}:{}: not removing {}: sources have unmapped imports {}",
            c.target,
            go_list(&c.kept),
            go_list(&c.unmapped)
        );
    }
}

/// Reports the targets of a rules.star file that sync skipped: always
/// those whose deps it can't read, and with -v those opted out with
/// `# turnkey:no-sync`
fn print_skipped_targets(
    rel_path: &str,
    result: &FileResult,
    verbose: bool,
    stderr: &mut dyn Write,
) {
    for u in &result.unreadable {
        let _ = writeln!(
            stderr,
            "UNREADABLE: {rel_path}:{}: {} is not a list of labels; write it as one, or add # turnkey:no-sync before the rule",
            u.target, u.attribute
        );
    }
    if verbose {
        for target in &result.opted_out {
            let _ = writeln!(stderr, "OPTED OUT: {rel_path}:{target}");
        }
    }
}

/// The help of `tk rules`
pub const HELP: &str = r##"Usage: tk rules <command> [options] [path]

Commands:
  check              Check every rules.star file against its sources
  sync               Update rules.star files with detected dependencies
  help               Show this help

Options:
  --all, -a          sync: process all files (skip staleness detection)
  --force, -f        Same as --all
  --verbose, -v      Show detailed output including skipped files
  --quiet, -q        Suppress output
  --dry-run, -n      Show what would be changed without writing

Staleness Detection:
  check always checks every rules.star file, so it also catches a stale
  file that is already committed. sync, by default, only processes
  directories with uncommitted changes whose source files are newer than
  rules.star. Use --all or --force to sync all files.

Examples:
  tk rules check                    # Check all rules.star files
  tk rules check src/cmd/tk         # Check one directory
  tk rules sync                     # Update stale rules.star files
  tk rules sync --all               # Force update all files
  tk rules sync src/cmd/tk          # Sync specific directory

The rules command automatically detects imports from source files and
updates the deps list in rules.star. Manual dependencies can be preserved
using turnkey:preserve-start/end markers, and a "# turnkey:no-sync" comment
before a rule leaves that target's deps alone."##;

fn print_help(stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "{HELP}");
}

/// The rules sync before a buck2 command, as .turnkey/sync.toml configures
/// it. Returns the exit code: not 0 only when strict mode finds a
/// rules.star that would change, or sync fails.
pub fn auto_sync(
    root: Result<&str, &str>,
    flags: &Flags,
    sync: RulesSync,
    stderr: &mut dyn Write,
) -> i32 {
    let (verbose, quiet) = (flags.verbose, flags.quiet);
    let root = match root {
        Ok(root) => root,
        Err(e) => {
            if verbose {
                let _ = writeln!(stderr, "tk: {e}");
            }
            // Don't fail if the project root can't be found
            return 0;
        }
    };
    let cfg = match SyncConfig::load_default_from(Path::new(root)) {
        Ok(cfg) => cfg,
        Err(e) => {
            if verbose {
                let _ = writeln!(stderr, "tk: could not load sync config for rules: {e:#}");
            }
            return 0;
        }
    };
    if !cfg.rules.enabled {
        return 0;
    }
    if verbose {
        let _ = writeln!(stderr, "tk: checking rules.star files...");
    }

    let strict = cfg.rules.strict || flags.strict_rules;
    let report = sync(Request {
        project_root: root.to_string(),
        dir: root.to_string(),
        // Dry run in strict mode
        dry_run: strict,
        verbose,
        force: false,
    });
    if let Some(e) = &report.error {
        if e.stage == Stage::Setup {
            if verbose {
                let _ = writeln!(stderr, "tk: could not create rules syncer: {}", e.message);
            }
            // Don't fail on syncer creation issues
            return 0;
        }
        let _ = writeln!(stderr, "tk: rules sync failed: {}", e.message);
        return 1;
    }
    let results = report.results.unwrap_or_default();

    let mut updated_count = 0;
    let mut unreadable = Vec::new();
    for result in &results {
        let rel_path = rel(root, &result.path);
        for u in &result.unreadable {
            unreadable.push(format!("{rel_path}:{}", u.target));
        }
        if !quiet {
            print_kept_deps(&rel_path, &result.changes, stderr);
        }
        if result.updated {
            updated_count += 1;
        }
    }
    let updated = || {
        results
            .iter()
            .filter(|r| r.updated)
            .map(|r| rel(root, &r.path))
    };

    // Strict mode (CI): a target whose deps sync can't read, and nothing
    // opts out, fails like a stale rules.star. Otherwise the check before a
    // build stays quiet about it: tk rules sync and tk rules check report
    // it.
    if strict && !unreadable.is_empty() {
        let _ = writeln!(
            stderr,
            "tk: {} target(s) have deps rules sync can't read (strict mode):",
            unreadable.len()
        );
        for u in &unreadable {
            let _ = writeln!(stderr, "  - {u}");
        }
        let _ = writeln!(
            stderr,
            "\ntk: write their deps as a list of labels, or add # turnkey:no-sync before the rule"
        );
        return 1;
    }

    if updated_count == 0 {
        if verbose && !quiet {
            let _ = writeln!(stderr, "tk: all rules.star files up-to-date");
        }
        return 0;
    }

    // Strict mode (CI): fail if any rules.star would change
    if strict {
        let _ = writeln!(
            stderr,
            "tk: {updated_count} rules.star file(s) need updates (strict mode):"
        );
        for path in updated() {
            let _ = writeln!(stderr, "  - {path}");
        }
        let _ = writeln!(
            stderr,
            "\ntk: run 'tk rules sync' locally and commit the changes"
        );
        return 1;
    }

    // Auto-sync disabled: just warn
    if !cfg.rules.is_auto_sync() {
        if !quiet {
            let _ = writeln!(
                stderr,
                "tk: {updated_count} rules.star file(s) need updates:"
            );
            for path in updated() {
                let _ = writeln!(stderr, "  - {path}");
            }
            let _ = writeln!(stderr, "tk: run 'tk rules sync' to update");
        }
        return 0;
    }

    // Auto-sync already happened: report it
    if !quiet {
        for path in updated() {
            let _ = writeln!(stderr, "tk: updated {path}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rules_syncer::report::UnreadableTarget;

    /// A rules sync that records its requests and returns report
    fn fake(report: Report, requests: &mut Vec<Request>) -> impl FnMut(Request) -> Report + '_ {
        move |req| {
            requests.push(req);
            report.clone()
        }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// A target whose deps sync can't read fails tk rules check; one a
    /// "# turnkey:no-sync" comment opts out doesn't
    #[test]
    fn check_fails_on_unreadable_deps() {
        let mut requests = Vec::new();
        let unreadable = Report {
            results: Some(vec![FileResult {
                path: "/project/lib/rules.star".into(),
                unreadable: vec![UnreadableTarget {
                    target: "lib".into(),
                    attribute: "deps".into(),
                }],
                ..Default::default()
            }]),
            error: None,
        };
        let mut stderr = Vec::new();
        let code = check(
            &mut fake(unreadable, &mut requests),
            "/project",
            &args(&["--all", "--quiet"]),
            &mut Flags::default(),
            &mut stderr,
        );
        assert_eq!(code, 1, "unreadable deps");

        let opted_out = Report {
            results: Some(vec![FileResult {
                path: "/project/lib/rules.star".into(),
                opted_out: vec!["lib".into()],
                ..Default::default()
            }]),
            error: None,
        };
        let code = check(
            &mut fake(opted_out, &mut requests),
            "/project",
            &args(&["--all", "--quiet"]),
            &mut Flags::default(),
            &mut stderr,
        );
        assert_eq!(code, 0, "opted out");
    }

    /// tk rules check checks every rules.star under the directory it is
    /// given, whatever their sources' file times and git status say: it
    /// asks rules sync for a forced dry run, with or without --all
    #[test]
    fn check_ignores_file_times() {
        for (a, dir) in [
            (args(&["--quiet"]), "/project"),
            (args(&["--all", "--quiet", "lib"]), "/project/lib"),
        ] {
            let mut requests = Vec::new();
            check(
                &mut fake(Report::default(), &mut requests),
                "/project",
                &a,
                &mut Flags::default(),
                &mut Vec::new(),
            );
            assert_eq!(
                requests,
                [Request {
                    project_root: "/project".into(),
                    dir: dir.into(),
                    dry_run: true,
                    verbose: false,
                    force: true,
                }],
                "{a:?}"
            );
        }
    }

    /// An error that stopped rules sync fails tk rules check
    #[test]
    fn check_fails_on_a_sync_error() {
        for stage in [Stage::Setup, Stage::Sync] {
            let report = Report {
                results: None,
                error: Some(Error {
                    stage,
                    message: "boom".into(),
                }),
            };
            let mut stderr = Vec::new();
            let code = check(
                &mut fake(report, &mut Vec::new()),
                "/project",
                &args(&["--quiet"]),
                &mut Flags::default(),
                &mut stderr,
            );
            assert_eq!(code, 1, "{stage:?}");
            let want = match stage {
                Stage::Setup => "tk rules: boom\n",
                Stage::Sync => "tk rules: check failed: boom\n",
            };
            assert_eq!(String::from_utf8(stderr).unwrap(), want);
        }
    }

    /// tk rules sync passes its flags on, and prints what changed
    #[test]
    fn sync_reports_what_it_changed() {
        let report = Report {
            results: Some(vec![
                FileResult {
                    path: "/project/a/rules.star".into(),
                    updated: true,
                    changes: vec![TargetChange {
                        target: "a".into(),
                        added: vec!["//b:b".into()],
                        removed: vec!["//c:c".into(), "//d:d".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FileResult {
                    path: "/project/b/rules.star".into(),
                    skipped: true,
                    ..Default::default()
                },
            ]),
            error: None,
        };
        let mut requests = Vec::new();
        let mut stderr = Vec::new();
        let mut flags = Flags::default();
        let code = sync_rules(
            &mut fake(report, &mut requests),
            "/project",
            &args(&["-n", "-v", "--all", "a"]),
            &mut flags,
            &mut stderr,
        );
        assert_eq!(code, 0);
        assert!(flags.dry_run && flags.verbose);
        assert_eq!(
            requests,
            [Request {
                project_root: "/project".into(),
                dir: "/project/a".into(),
                dry_run: true,
                verbose: true,
                force: true,
            }]
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "WOULD UPDATE: a/rules.star
       :a
         Added: [//b:b]
         Removed: [//c:c //d:d]
SKIPPED: b/rules.star (up-to-date)

tk rules: would update 1 file(s), skipped 1 (up-to-date)
"
        );
    }

    /// Before a build, strict mode fails on a rules.star that would change,
    /// and auto-sync reports the files it updated
    #[test]
    fn auto_sync_follows_the_sync_config() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap().to_string();
        std::fs::create_dir_all(tmp.path().join(".turnkey")).unwrap();
        let updated = Report {
            results: Some(vec![FileResult {
                path: format!("{root}/a/rules.star"),
                updated: true,
                ..Default::default()
            }]),
            error: None,
        };
        let run = |config: &str, flags: Flags, report: &Report| {
            std::fs::write(tmp.path().join(".turnkey/sync.toml"), config).unwrap();
            let mut requests = Vec::new();
            let mut stderr = Vec::new();
            let code = auto_sync(
                Ok(&root),
                &flags,
                &mut fake(report.clone(), &mut requests),
                &mut stderr,
            );
            (code, requests, String::from_utf8(stderr).unwrap())
        };

        // Disabled: nothing runs
        let (code, requests, _) = run("", Flags::default(), &updated);
        assert_eq!((code, requests.len()), (0, 0));

        let (code, requests, stderr) = run("[rules]\nenabled = true\n", Flags::default(), &updated);
        assert_eq!(code, 0);
        assert!(!requests[0].dry_run && !requests[0].force);
        assert_eq!(stderr, "tk: updated a/rules.star\n");

        let strict = Flags {
            strict_rules: true,
            ..Default::default()
        };
        let (code, requests, stderr) = run("[rules]\nenabled = true\n", strict, &updated);
        assert_eq!(code, 1);
        assert!(requests[0].dry_run);
        assert!(stderr.contains("need updates (strict mode):\n  - a/rules.star\n"));

        let (code, _, stderr) = run(
            "[rules]\nenabled = true\nauto_sync = false\n",
            Flags::default(),
            &updated,
        );
        assert_eq!(code, 0);
        assert!(stderr.ends_with("tk: run 'tk rules sync' to update\n"));

        // A setup error doesn't fail the build; a sync error does
        for (stage, want) in [(Stage::Setup, 0), (Stage::Sync, 1)] {
            let report = Report {
                results: None,
                error: Some(Error {
                    stage,
                    message: "boom".into(),
                }),
            };
            let (code, _, _) = run("[rules]\nenabled = true\n", Flags::default(), &report);
            assert_eq!(code, want, "{stage:?}");
        }
    }
}
