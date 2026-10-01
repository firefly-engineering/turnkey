//! tk is the turnkey CLI wrapper for buck2.
//!
//! It runs sync (deps files, then rules.star files) before the buck2
//! commands that read the build graph, ensuring generated files are
//! up-to-date, and passes everything to buck2, rewriting its command line on
//! the way (buck2-args).
//!
//! ```text
//! tk build //some:target     # syncs first, then runs buck2 build
//! tk test //some:target      # syncs first, then runs buck2 test
//! tk clean                   # passes through directly (no sync)
//! tk --no-sync build ...     # skip sync, run buck2 directly
//! ```
//!
//! The Rust port of src/cmd/tk (#216). What tk reads of its process (its
//! arguments, environment and working directory) is read once, here, and
//! handed down.

mod buck2;
mod completion;
mod compose;
mod flags;
mod help;
mod materialize;
mod rules;
mod sync;

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use flags::{Flags, Parsed, RuleArgsError};
use project_sync::config::{Config as SyncConfig, find_root};
use project_sync::launch::{self, Launcher};

/// What tk reads of its process: its environment and working directory
pub struct Env {
    /// The environment; the first of two definitions of a variable wins,
    /// as with getenv
    vars: HashMap<OsString, OsString>,
    /// Starts children, finding them on the environment's `PATH`
    launcher: Launcher,
    /// The working directory, or why it can't be read
    cwd: Result<PathBuf, String>,
}

impl Env {
    /// An environment of vars, in the working directory cwd
    pub fn new(
        vars: impl IntoIterator<Item = (OsString, OsString)>,
        cwd: Result<PathBuf, String>,
    ) -> Env {
        let mut map = HashMap::new();
        for (key, value) in vars {
            map.entry(key).or_insert(value);
        }
        Env {
            launcher: Launcher::new(map.get(OsStr::new("PATH")).cloned()),
            vars: map,
            cwd,
        }
    }

    /// `os.Getenv`: a variable's value, as text
    fn getenv(&self, key: &str) -> Option<String> {
        self.vars
            .get(OsStr::new(key))
            .map(|v| v.to_string_lossy().into_owned())
    }

    /// The project root above the working directory (find_root), or the
    /// working directory itself outside a project: buck2 then reports the
    /// missing .buckconfig, and sync finds no sync.toml and so nothing to
    /// do.
    fn project_root(&self) -> Result<PathBuf, String> {
        let cwd = self
            .cwd
            .as_ref()
            .map_err(|e| format!("failed to get working directory: {e}"))?;
        Ok(find_root(cwd).unwrap_or_else(|| cwd.clone()))
    }

    /// The working directory as text, "" when it can't be read
    fn cwd_string(&self) -> String {
        self.cwd
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cwd = launch::getwd(std::env::var_os("PWD").as_deref()).map_err(|e| e.to_string());
    let env = Env::new(std::env::vars_os(), cwd);
    std::process::exit(run(&args, &env));
}

fn print_help() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(help::HELP.as_bytes());
    let _ = stdout.flush();
}

/// Runs tk with its command-line args, and returns its exit code (unless
/// it replaces itself with buck2)
fn run(args: &[String], env: &Env) -> i32 {
    let mut flags = Flags::default();
    let args = match flags.parse(args) {
        Parsed::Help => {
            print_help();
            return 0;
        }
        Parsed::Rest(rest) => rest,
    };
    let Some((subcommand, rest)) = args.split_first() else {
        print_help();
        return 0;
    };
    let stderr = &mut std::io::stderr();

    match subcommand.as_str() {
        "sync" | "check" => {
            let only = match flags.rule_args(rest) {
                Ok(only) => only,
                Err(RuleArgsError::Help) => {
                    print_help();
                    return 0;
                }
                Err(RuleArgsError::UnknownFlag(arg)) => {
                    let _ = writeln!(stderr, "tk: unknown flag {}", gostd::strconv::quote(&arg));
                    return 2;
                }
            };
            let regenerate = subcommand == "sync";
            return sync::run_deps(env, &flags, regenerate, only, true);
        }
        "rules" => {
            let find_root = || env.project_root().map(|r| r.to_string_lossy().into_owned());
            let mut syncer = rules::syncer(env.launcher.clone(), env.cwd_string());
            return rules::run(rest, &find_root, &mut flags, &mut syncer, stderr);
        }
        "compose" => {
            let find_root = || env.project_root();
            let mut stdout = std::io::stdout();
            let mut stdin = std::io::stdin().lock();
            return compose::run(
                rest,
                &find_root,
                flags.verbose,
                &mut compose::Io {
                    stdout: &mut stdout,
                    stderr,
                    stdin: &mut stdin,
                },
            );
        }
        "completion" => return buck2::completion(rest, env, stderr),
        "materialize" => {
            if rest.is_empty() {
                let _ = writeln!(stderr, "{}", materialize::USAGE);
                return 1;
            }
            let root = match env.project_root() {
                Ok(root) => root,
                Err(e) => {
                    let _ = writeln!(stderr, "tk: {e}");
                    return 1;
                }
            };
            let mut add_root =
                |link: &Path, store_path: &str| buck2::add_gc_root(env, link, store_path);
            return materialize::run(
                rest,
                &root,
                &mut add_root,
                flags.verbose,
                flags.quiet,
                stderr,
            );
        }
        _ => {}
    }

    // Whether this command needs sync first, from buck2's subcommand (past
    // its universal options)
    let buck2_subcommand = buck2_args::subcommand(args).map_or("", |(s, _)| s);
    let needs_sync = buck2_args::needs_sync(buck2_subcommand);

    if needs_sync && !flags.no_sync {
        if flags.verbose {
            let _ = writeln!(stderr, "tk: syncing before {buck2_subcommand}...");
        }
        let code = sync::run_deps(env, &flags, true, Vec::new(), false);
        if code != 0 {
            return code;
        }
    }

    // Rules sync, if enabled: after deps sync, before buck2
    if needs_sync && !flags.no_rules_sync {
        let root = env.project_root().map(|r| r.to_string_lossy().into_owned());
        let mut syncer = rules::syncer(env.launcher.clone(), env.cwd_string());
        let code = rules::auto_sync(
            root.as_deref().map_err(String::as_str),
            &flags,
            &mut syncer,
            stderr,
        );
        if code != 0 {
            return code;
        }
    }

    // Whether cell symlinks changed (which kills the daemon), and whether a
    // materialized cell lags its deps file
    if needs_sync && !flags.no_sync {
        check_cell_freshness(env, &flags, stderr);
        warn_stale_cells(env, &flags, stderr);
    }

    buck2::delegate(args, env, &flags)
}

/// Kills the buck2 daemon when the cells' symlinks changed, so it picks up
/// the new store paths. Best effort.
fn check_cell_freshness(env: &Env, flags: &Flags, stderr: &mut dyn Write) {
    let Ok(root) = env.project_root() else {
        return;
    };
    let mut kill = || buck2::kill_daemon(env);
    if let Err(e) =
        deps_cells::freshness::check(&root, flags.verbose, flags.quiet, stderr, &mut kill)
        && flags.verbose
    {
        let _ = writeln!(stderr, "tk: cell freshness check failed: {e}");
    }
}

/// Warns when a materialized deps cell was built from another version of
/// its deps file than the one on disk: the shell hasn't re-evaluated since
/// the file changed
fn warn_stale_cells(env: &Env, flags: &Flags, stderr: &mut dyn Write) {
    let Ok(root) = env.project_root() else {
        return;
    };
    let Ok(cfg) = SyncConfig::load_default_from(&root) else {
        return;
    };
    let cells = cfg
        .languages
        .iter()
        .map(|l| (l.cell.as_str(), l.deps_file.as_str()));
    materialize::warn_stale_cells(&root, cells, flags.quiet, stderr);
}
