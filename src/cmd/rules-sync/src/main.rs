//! rules-sync keeps the deps of a project's rules.star files in step with
//! their sources (the rules-syncer crate's syncer), and prints what it did as
//! a JSON report on stdout.
//!
//! tk runs it for `tk rules check`, `tk rules sync` and the rules sync
//! before a buck2 command, and prints the report its own way. Its verbose
//! messages go to stderr. It exits 0 when sync ran, 1 when an error stopped
//! it (the report's error says where), and 2 on bad usage.
//!
//! Usage:
//!
//! ```text
//! rules-sync --project-root <root> [--dry-run] [--verbose] [--force] [dir]
//! ```
//!
//! dir is the directory whose rules.star files are synced, the project root
//! by default. Ported from Go (#215): the flags are parsed as Go's flag
//! package parses them, the report is written as Go's encoding/json writes
//! it, and the exit codes are the Go version's. Once tk is Rust, rules sync
//! goes back into tk.

use project_sync::launch::Launcher;
use rules_syncer::report::{Error, Report, Stage};
use rules_syncer::sync::{Config, Syncer};
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = Env {
        path: std::env::var_os("PATH"),
        cwd: std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let code = run(
        &args,
        &env,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code);
}

/// What rules-sync reads of its process: the PATH it finds the tools it
/// runs on, and its working directory
pub struct Env {
    pub path: Option<std::ffi::OsString>,
    pub cwd: String,
}

const USAGE: &str =
    "Usage: rules-sync --project-root <root> [--dry-run] [--verbose] [--force] [dir]
  -dry-run
    \treport what would change without writing
  -force
    \tsync every rules.star, not only those whose sources changed
  -project-root string
    \tthe project's root, where .turnkey/sync.toml is (required)
  -verbose
    \tprint what sync does on stderr
";

/// Runs rules-sync with the command-line `args`, and returns its exit code.
pub fn run(args: &[String], env: &Env, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let flags = match parse_flags(args) {
        Ok(flags) => flags,
        Err(err) => {
            if let Some(err) = err {
                let _ = writeln!(stderr, "{err}");
            }
            let _ = write!(stderr, "{USAGE}");
            return 2;
        }
    };
    if flags.project_root.is_empty() || flags.args.len() > 1 {
        let _ = write!(stderr, "{USAGE}");
        return 2;
    }
    let dir = flags
        .args
        .first()
        .cloned()
        .unwrap_or_else(|| flags.project_root.clone());
    let cfg = Config {
        project_root: flags.project_root,
        dry_run: flags.dry_run,
        verbose: flags.verbose,
        force: flags.force,
        sync: None,
        launcher: Launcher::new(env.path.clone()),
        cwd: env.cwd.clone(),
    };

    let report = sync_rules(cfg, &dir);
    if let Err(err) = stdout.write_all(report.to_json().as_bytes()) {
        let _ = writeln!(stderr, "rules-sync: writing the report: {err}");
        return 1;
    }
    if report.error.is_some() { 1 } else { 0 }
}

/// Syncs the rules.star files under `dir`, and reports what it did.
fn sync_rules(cfg: Config, dir: &str) -> Report {
    let failed = |stage, message: String| Report {
        results: None,
        error: Some(Error { stage, message }),
    };
    let syncer = match Syncer::new(cfg) {
        Ok(syncer) => syncer,
        Err(err) => return failed(Stage::Setup, format!("{err:#}")),
    };
    match syncer.sync_directory(dir) {
        Ok(results) => Report {
            results: Some(results),
            error: None,
        },
        Err(err) => failed(Stage::Sync, format!("{err:#}")),
    }
}

/// The command line, as Go's flag package parses it
#[derive(Debug, Default, PartialEq, Eq)]
struct Flags {
    project_root: String,
    dry_run: bool,
    verbose: bool,
    force: bool,
    /// The arguments after the flags
    args: Vec<String>,
}

/// Parses the flags as Go's `flag.FlagSet.Parse` does: `-name` or
/// `--name`, a value after `=` or (for a string flag) as the next
/// argument, up to the first argument that isn't a flag or after `--`.
/// `Err(None)` is a request for help (`-h`, `-help`), `Err(Some(..))` a
/// usage error.
fn parse_flags(args: &[String]) -> Result<Flags, Option<String>> {
    let mut flags = Flags::default();
    let mut i = 0;
    while i < args.len() {
        let s = &args[i];
        if s.len() < 2 || !s.starts_with('-') {
            break;
        }
        let minuses = if s.as_bytes()[1] == b'-' {
            if s.len() == 2 {
                // "--" terminates the flags
                i += 1;
                break;
            }
            2
        } else {
            1
        };
        let name = &s[minuses..];
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(Some(format!("bad flag syntax: {s}")));
        }
        i += 1;
        // equals cannot be first
        let (name, value) = match name[1..].find('=') {
            Some(eq) => (&name[..eq + 1], Some(&name[eq + 2..])),
            None => (name, None),
        };
        let target = match name {
            "project-root" => {
                let value = match value {
                    Some(value) => value.to_string(),
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return Err(Some(format!("flag needs an argument: -{name}"))),
                };
                flags.project_root = value;
                continue;
            }
            "dry-run" => &mut flags.dry_run,
            "verbose" => &mut flags.verbose,
            "force" => &mut flags.force,
            "help" | "h" => return Err(None),
            _ => return Err(Some(format!("flag provided but not defined: -{name}"))),
        };
        *target = match value {
            None => true,
            Some(value) => parse_bool(value).ok_or_else(|| {
                Some(format!(
                    "invalid boolean value {} for -{name}: parse error",
                    gostd::strconv::quote(value)
                ))
            })?,
        };
    }
    flags.args = args[i..].to_vec();
    Ok(flags)
}

/// `strconv.ParseBool`
fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
