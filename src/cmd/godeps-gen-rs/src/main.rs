//! godeps-gen generates go-deps.toml from a Go workspace: the members of
//! the project's go.work, or, without one, its go.mod (ADR 0007).
//!
//! The modules are those the members require, minus the members; their
//! versions are the build list `go list -m -json all` selects for the
//! whole workspace. go-deps.toml records the members and the files it was
//! generated from (go.work, go.work.sum, each member's go.mod and go.sum).
//! This tool outputs dependency declarations in the format expected by
//! turnkey's Go deps cell (nix/buck2/languages.nix).
//!
//! Its --output, --no-prefetch and --no-cache flags are the ones every deps
//! generator takes (src/rust/deps-gen-kit): prefetching is on by default.
//! tk sync runs it from the go sync rule (nix/buck2/languages.nix), in the
//! project root.
//!
//! It is the Rust port of src/cmd/godeps-gen (#211), and writes the same
//! bytes.
//!
//! Usage:
//!
//! ```text
//! godeps-gen -o go-deps.toml
//! godeps-gen --go-work go.work --go-mod go.mod --go-sum go.sum -o go-deps.toml
//! ```

mod deps;
mod output;
mod prefetch;
mod workspace;

use clap::{ArgAction, Parser};
use deps::ParseOptions;
use deps_gen_kit::{OutputArgs, PrefetchArgs};
use output::{DepsFile, OutputOptions};
use prefetch::{GoProxyPrefetcher, prefetch_all};
use std::process::ExitCode;
use workspace::{GoLister, Workspace};

/// Generate go-deps.toml from a Go module or workspace
#[derive(Parser, Debug)]
#[command(name = "godeps-gen", args_override_self = true)]
struct Args {
    /// Path to go.work; when it exists, its members are the workspace and
    /// --go-mod and --go-sum are unused
    #[arg(long = "go-work", default_value = "go.work")]
    go_work: String,

    /// Path to go.mod file, the only member without a go.work
    #[arg(long = "go-mod", default_value = "go.mod")]
    go_mod: String,

    /// Path to go.sum file, with --go-mod
    #[arg(long = "go-sum", default_value = "go.sum")]
    go_sum: String,

    #[command(flatten)]
    output: OutputArgs,

    #[command(flatten)]
    prefetch: PrefetchArgs,

    /// Include indirect (transitive) dependencies (--indirect=false to
    /// leave them out)
    #[arg(
        long,
        default_value_t = true,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        action = ArgAction::Set,
    )]
    indirect: bool,

    /// Arguments after the flags, ignored as Go's flag package ignores them
    #[arg(hide = true, trailing_var_arg = true)]
    _rest: Vec<String>,
}

fn main() -> ExitCode {
    let mut args = match Args::try_parse() {
        Ok(args) => args,
        // Go's flag package prints its usage to stderr, -h included
        Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => {
            eprint!("{}", e.render());
            return ExitCode::SUCCESS;
        }
        Err(e) => e.exit(),
    };
    // -o "" is stdout, as without -o
    if args
        .output
        .output
        .as_ref()
        .is_some_and(|p| p.as_os_str().is_empty())
    {
        args.output.output = None;
    }

    // File paths are relative to the working directory, the project root
    let root = match std::env::current_dir() {
        Ok(dir) => match dir.into_os_string().into_string() {
            Ok(root) => root,
            Err(dir) => return fail(format!("error: the working directory {dir:?} is not UTF-8")),
        },
        Err(e) => return fail(format!("error: {e}")),
    };

    let ws = match Workspace::load(&root, &args.go_work, &args.go_mod, &args.go_sum) {
        Ok(ws) => ws,
        Err(e) => return fail(format!("error reading the Go workspace: {e:#}")),
    };

    let lister = GoLister {
        go: "go".into(),
        env: std::env::vars_os().collect(),
    };
    let opts = ParseOptions {
        include_indirect: args.indirect,
    };
    let mut deps = match ws.resolve(&root, &lister, opts) {
        Ok(deps) => deps,
        Err(e) => return fail(format!("error resolving the Go workspace: {e:#}")),
    };

    // Prefetch Nix hashes unless asked not to
    if !args.prefetch.no_prefetch {
        eprintln!("Prefetching {} dependencies...", deps.len());
        let mut prefetcher = GoProxyPrefetcher::new(args.prefetch.no_cache);
        prefetch_all(&mut deps, &mut prefetcher, |dep, err| {
            eprintln!("warning: failed to prefetch {}: {}", dep.import_path, err);
        });
    }

    let file = DepsFile {
        deps,
        sources: ws.sources.clone(),
        members: ws.members.clone(),
    };
    let content = output::render(&file, OutputOptions::all());
    match args.output.write_text(&content) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(format!("error writing output: {e:#}")),
    }
}

/// Exit 1 after printing `message`
fn fail(message: String) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_as_go_parses_them() {
        let args = Args::try_parse_from(["godeps-gen"]).unwrap();
        assert_eq!(
            (
                args.go_work.as_str(),
                args.go_mod.as_str(),
                args.go_sum.as_str()
            ),
            ("go.work", "go.mod", "go.sum")
        );
        assert!(args.indirect && !args.prefetch.no_prefetch && args.output.output.is_none());

        let args = Args::try_parse_from([
            "godeps-gen",
            "--go-work=w",
            "--go-mod",
            "m",
            "--go-sum",
            "s",
            "--indirect=false",
            "--no-prefetch",
            "-o",
            "a.toml",
            "--output",
            "b.toml",
        ])
        .unwrap();
        assert_eq!(
            (
                args.go_work.as_str(),
                args.go_mod.as_str(),
                args.go_sum.as_str()
            ),
            ("w", "m", "s")
        );
        assert!(!args.indirect && args.prefetch.no_prefetch);
        // The last of repeated flags wins
        assert_eq!(args.output.output.unwrap().to_str(), Some("b.toml"));

        assert!(
            Args::try_parse_from(["godeps-gen", "--indirect"])
                .unwrap()
                .indirect
        );
        // Flag parsing stops at the first argument
        let args = Args::try_parse_from(["godeps-gen", "x", "--no-prefetch"]).unwrap();
        assert!(!args.prefetch.no_prefetch);

        let err = Args::try_parse_from(["godeps-gen", "--no-prefetch", "--no-cache"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(
            Args::try_parse_from(["godeps-gen", "--bogus"])
                .unwrap_err()
                .exit_code(),
            2
        );
    }
}
