//! What tk runs of buck2 and nix-store: the buck2 command it delegates to,
//! buck2's completion scripts, the daemon kill when cells change, and the
//! GC roots of materialized cells

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use project_sync::launch;

use crate::Env;
use crate::completion;
use crate::flags::Flags;

/// Go's `%v` of a []string: `[a b]`
fn go_list(items: &[String]) -> String {
    format!("[{}]", items.join(" "))
}

/// Runs buck2 with args, rewritten as tk rewrites buck2's command line
/// (the isolation directory, the local target overrides), in place of tk:
/// tk replaces itself with buck2, except for `test` with the test result
/// cache, where it stays to report the results reused. Returns tk's exit
/// code when buck2 doesn't replace it.
pub fn delegate(args: &[String], env: &Env, flags: &Flags) -> i32 {
    let mut stderr = std::io::stderr();
    let buck2 = match env.launcher.look_path(OsStr::new("buck2")) {
        Ok(path) => path,
        Err(e) => {
            let _ = writeln!(stderr, "tk: buck2 not found in PATH: {e}");
            return 1;
        }
    };

    // The isolation directory gets a .turnkey prefix
    let mut args = buck2_args::transform_isolation_dir(args);
    if !flags.no_local {
        args = apply_local_overrides(env, flags, args, &mut stderr);
    }

    // Tests run with the test result cache when the dev shell has one. tk
    // stays around to print how many results were reused once buck2's own
    // summary is out.
    if buck2_args::subcommand(&args).is_some_and(|(s, _)| s == "test") {
        let getenv = |key: &str| env.getenv(key);
        match testcache::Config::from_env(&getenv) {
            Err(e) => {
                let _ = writeln!(
                    stderr,
                    "tk: running tests without the test result cache: {e}"
                );
            }
            Ok(Some(cache)) => {
                let mut run = |args: &[String]| run_child(&buck2, args, flags.verbose);
                return cache.run_tests(&args, flags.rerun, &getenv, &mut run, &mut stderr);
            }
            Ok(None) => {}
        }
    }

    if flags.verbose {
        let _ = writeln!(stderr, "tk: executing buck2 {}", go_list(&args));
    }
    // Replace this process with buck2, so that signals, stdio and the exit
    // code are buck2's own
    let err = Command::new(&buck2).arg0("buck2").args(&args).exec();
    let _ = writeln!(stderr, "tk: failed to exec buck2: {err}");
    1
}

/// Runs buck2 as a child of tk, for a caller that has more to do once it
/// exits, and returns its exit code as a shell reports it
fn run_child(buck2: &Path, args: &[String], verbose: bool) -> i32 {
    if verbose {
        eprintln!("tk: executing buck2 {}", go_list(args));
    }
    let mut cmd = Command::new(buck2);
    cmd.arg0("buck2").args(args);
    // ^C reaches buck2 directly through the terminal's process group;
    // forwarding it too would read as a second ^C. Other termination
    // signals are passed on.
    match launch::run_leaving_interrupts(&mut cmd) {
        Ok(status) => launch::shell_exit_code(status),
        Err(e) => {
            eprintln!("tk: failed to run buck2: {e}");
            1
        }
    }
}

/// Injects the arguments .turnkey/local.toml has for the command line's
/// target, if any
fn apply_local_overrides(
    env: &Env,
    flags: &Flags,
    args: Vec<String>,
    stderr: &mut dyn Write,
) -> Vec<String> {
    if args.is_empty() {
        return args;
    }
    let root = match env.project_root() {
        Ok(root) => root,
        Err(e) => {
            if flags.verbose {
                let _ = writeln!(
                    stderr,
                    "tk: could not find project root for local config: {e}"
                );
            }
            return args;
        }
    };
    let config = match buck2_args::LocalConfig::load_default_from(&root) {
        Ok(config) => config,
        Err(e) => {
            if flags.verbose {
                let _ = writeln!(stderr, "tk: could not load local config: {e}");
            }
            return args;
        }
    };
    match buck2_args::apply_local_overrides(&config, &args) {
        Some(applied) => {
            if flags.verbose {
                let _ = writeln!(
                    stderr,
                    "tk: applying local override for {} {}: {}",
                    applied.subcommand,
                    applied.target,
                    go_list(&applied.override_args)
                );
            }
            applied.args
        }
        None => args,
    }
}

/// `tk completion <shell>`: buck2's completion script for the shell, made
/// tk's
pub fn completion(args: &[String], env: &Env, stderr: &mut dyn Write) -> i32 {
    let shell = match completion::parse_args(args) {
        Ok(shell) => shell,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 1;
        }
    };
    let output = env
        .launcher
        .command(OsStr::new("buck2"), &["completion", shell.as_str()], None)
        .map_err(|e| e.to_string())
        .and_then(|mut cmd| cmd.output().map_err(|e| e.to_string()))
        .and_then(|out| {
            if out.status.success() {
                Ok(out.stdout)
            } else {
                Err(launch::exit_error(out.status))
            }
        });
    match output {
        Ok(script) => {
            let script = completion::transform(&String::from_utf8_lossy(&script), shell);
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(script.as_bytes());
            let _ = stdout.flush();
            0
        }
        Err(e) => {
            let _ = writeln!(stderr, "tk: failed to get buck2 completion: {e}");
            1
        }
    }
}

/// Kills the buck2 daemon (`buck2 kill`, its output on tk's stderr), so
/// that it picks up new cells. Best effort.
pub fn kill_daemon(env: &Env) {
    let Ok(mut cmd) = env.launcher.command(OsStr::new("buck2"), &["kill"], None) else {
        return;
    };
    cmd.stdin(Stdio::null())
        .stdout(std::io::stderr())
        .stderr(std::io::stderr());
    let _ = cmd.status();
}

/// Registers link as a GC root for store_path, through the Nix daemon:
/// only it can write /nix/var/nix/gcroots. Nothing is built; the path is
/// already in the store.
pub fn add_gc_root(env: &Env, link: &Path, store_path: &str) -> Result<(), String> {
    let args = [
        OsStr::new("--add-root"),
        link.as_os_str(),
        OsStr::new("--realise"),
        OsStr::new(store_path),
    ];
    let cmd = env
        .launcher
        .command(OsStr::new("nix-store"), &args, None)
        .map_err(|e| format!("nix-store --add-root: {e}\n"))?;
    let (status, out) = combined_output(cmd).map_err(|e| format!("nix-store --add-root: {e}\n"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "nix-store --add-root: {}\n{}",
        launch::exit_error(status),
        String::from_utf8_lossy(&out)
    ))
}

/// Runs cmd with its stdout and stderr on one pipe, as Go's
/// `CombinedOutput` does, and returns its status and what it wrote
fn combined_output(mut cmd: Command) -> std::io::Result<(std::process::ExitStatus, Vec<u8>)> {
    let (mut reader, writer) = std::io::pipe()?;
    cmd.stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = cmd.spawn()?;
    // The command holds the pipe's write ends: drop them, or the read
    // never ends
    drop(cmd);
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok((child.wait()?, out))
}
