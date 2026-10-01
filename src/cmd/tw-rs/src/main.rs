//! tw is the turnkey wrapper for native language tools.
//!
//! It transparently wraps tools like go, cargo, and uv, detecting when they
//! modify dependency files and automatically triggering sync operations.
//!
//! ```text
//! tw go get github.com/foo/bar    # runs go get, syncs if go.mod changed
//! tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
//! tw uv add requests              # runs uv add, syncs if pyproject.toml changed
//! ```
//!
//! Wrapper rules are read from .turnkey/sync.toml, which turnkey generates
//! from its language records (nix/buck2/languages.nix):
//!
//! ```toml
//! [[wrappers]]
//! name = "go"
//! command = "go"
//! mutating_subcommands = ["get", "mod"]
//! watch_files = ["go.mod", "go.sum"]
//! deps_rule = "go"
//! ```
//!
//! A tool without a wrapper rule is passed through untouched.
//!
//! This is the Rust port of `src/cmd/tw` (#214).

mod snapshot;
mod wrap;

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use wrap::{Log, RealExec, Wrapper};

const HELP: &str = r#"tw - turnkey wrapper for native language tools

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
"#;

/// tw's own flags, which come before the tool
#[derive(Debug, Default, PartialEq, Eq)]
struct Flags {
    verbose: bool,
    no_sync: bool,
    help: bool,
}

/// Splits tw's flags off the beginning of args; what's left is the tool
/// and its arguments. Parsing stops at help.
fn parse_flags(mut args: &[OsString]) -> (Flags, &[OsString]) {
    let mut flags = Flags::default();
    while let Some((first, rest)) = args.split_first() {
        match first.to_str() {
            Some("--verbose" | "-v") => flags.verbose = true,
            Some("--no-sync") => flags.no_sync = true,
            Some("--help" | "-h") => {
                flags.help = true;
                return (flags, rest);
            }
            _ => return (flags, args),
        }
        args = rest;
    }
    (flags, args)
}

/// `os.Getwd`: the working directory, named as `$PWD` names it when that
/// is the same directory (a path through a symbolic link, as the shell
/// shows it), else as the system does
fn getwd(pwd: Option<&OsStr>) -> std::io::Result<PathBuf> {
    let dot = std::fs::metadata(".")?;
    if let Some(dir) = pwd.filter(|d| d.as_encoded_bytes().first() == Some(&b'/'))
        && let Ok(d) = std::fs::metadata(dir)
        && d.dev() == dot.dev()
        && d.ino() == dot.ino()
    {
        return Ok(PathBuf::from(dir));
    }
    std::env::current_dir()
}

fn print_help() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(HELP.as_bytes());
    let _ = stdout.flush();
}

fn main() {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (flags, args) = parse_flags(&argv);
    if flags.help {
        print_help();
        std::process::exit(0);
    }
    let Some((tool, tool_args)) = args.split_first() else {
        print_help();
        std::process::exit(0);
    };

    let real = RealExec::new(std::env::vars_os());
    let launcher = real.launcher();
    let cwd = match getwd(std::env::var_os("PWD").as_deref()) {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("tw: failed to get working directory: {e}");
            std::process::exit(real.run(None, tool, tool_args));
        }
    };
    let exec: wrap::Exec = Box::new(move |dir, tool, args| real.run(dir, tool, args));
    let mut w = Wrapper::open(&cwd, exec, Log::new(std::io::stderr()), launcher);
    w.verbose = flags.verbose;
    w.no_sync = flags.no_sync;
    let code = w.run(tool, tool_args);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn flags_come_before_the_tool() {
        let argv = args(&["-v", "--no-sync", "go", "-v", "get"]);
        let (flags, rest) = parse_flags(&argv);
        assert_eq!(
            flags,
            Flags {
                verbose: true,
                no_sync: true,
                help: false
            }
        );
        assert_eq!(rest, args(&["go", "-v", "get"]));

        // An unknown flag is the tool
        let argv = args(&["--frobnicate", "go"]);
        assert_eq!(parse_flags(&argv), (Flags::default(), &argv[..]));

        let argv = args(&["--verbose", "-h", "go"]);
        assert!(parse_flags(&argv).0.help);
        let argv = args(&["--verbose"]);
        assert!(parse_flags(&argv).1.is_empty());
    }

    #[test]
    fn getwd_takes_pwd_only_when_it_names_the_working_directory() {
        let physical = std::env::current_dir().unwrap();
        assert_eq!(getwd(None).unwrap(), physical);
        assert_eq!(getwd(Some(OsStr::new("relative"))).unwrap(), physical);
        // Another directory
        assert_eq!(getwd(Some(OsStr::new("/"))).unwrap(), physical);
        // Not cleaned: the root finder cleans it
        let unclean = physical.join(".");
        assert_eq!(getwd(Some(unclean.as_os_str())).unwrap(), unclean);
    }
}
