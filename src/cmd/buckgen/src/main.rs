//! buckgen writes the rules.star files of one Go module, in place.
//!
//! It runs inside each Go module's derivation (`mkGoDepPackage` in
//! nix/lib/deps-cell/adapters/go.nix), on the module's own source, and
//! writes one rules.star per Go package: a `go_library` whose deps are
//! resolved per platform and allowed build tag (src/rust/goparse,
//! src/rust/conditions).
//!
//! It was ported from Go (#213), and writes the bytes the Go version
//! wrote.
//!
//! Usage:
//!
//! ```text
//! buckgen --config <buckgen.json> --module-path <module path> \
//!   --targets-out <file> --imports-out <file> <module-dir>
//! ```
//!
//! It writes, one per line, each Go package it rendered as
//! `<subdir> <target>` ("." for the module's root) to --targets-out, and
//! the non-stdlib import paths their deps reference to --imports-out.

mod config;
mod flags;
mod render;

use std::process::ExitCode;

const USAGE: &str = "Usage: buckgen --config <json> --module-path <path> --targets-out <file> --imports-out <file> <module-dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let flags = match flags::parse(&args) {
        Ok(flags) => flags,
        Err(flags::Error::Help) => {
            eprintln!("{}", flags::usage());
            return ExitCode::SUCCESS;
        }
        Err(flags::Error::Invalid(msg)) => {
            eprintln!("{msg}\n{}", flags::usage());
            return ExitCode::from(2);
        }
    };
    if flags.args.len() != 1
        || flags.config.is_empty()
        || flags.module_path.is_empty()
        || flags.targets_out.is_empty()
        || flags.imports_out.is_empty()
    {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    // A Go panic exits 2: so does a panic here
    match std::panic::catch_unwind(|| run(&flags)) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(1)
        }
        Err(_) => ExitCode::from(2),
    }
}

fn run(flags: &flags::Flags) -> Result<(), String> {
    let cfg = std::fs::read(&flags.config)
        .map_err(|e| e.to_string())
        .and_then(|data| config::load(&data).map_err(|e| e.to_string()))
        .map_err(|e| format!("Error loading config: {e}"))?;

    let (rendered, imports) = render::render_module(&flags.args[0], &flags.module_path, &cfg)
        .map_err(|e| format!("Error rendering {}: {e}", flags.module_path))?;

    let targets: String = rendered
        .iter()
        .map(|pkg| format!("{} {}\n", pkg.subdir, pkg.target))
        .collect();
    render::write_file(&flags.targets_out, targets.as_bytes())
        .map_err(|e| format!("Error writing {}: {e}", flags.targets_out))?;
    let lines: String = imports.iter().map(|imp| format!("{imp}\n")).collect();
    render::write_file(&flags.imports_out, lines.as_bytes())
        .map_err(|e| format!("Error writing {}: {e}", flags.imports_out))
}
