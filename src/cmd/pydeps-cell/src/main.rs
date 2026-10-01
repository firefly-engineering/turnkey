//! pydeps-cell writes the rules.star files of the pydeps cell's vendored
//! packages, with the dependencies between them evaluated per platform.
//!
//! It runs inside the Python deps-cell derivation
//! (nix/lib/deps-cell/adapters/python.nix), once the packages are merged
//! into the cell.
//!
//! It was ported from Go (#212), and writes the bytes the Go version
//! wrote.
//!
//! Usage:
//!
//! ```text
//! pydeps-cell <cell-dir> <python-deps.toml>
//! ```
//!
//! with the platforms and the Python version in
//! `<cell-dir>/pydeps-cell.json`.

mod cell;
mod decode;

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.len() != 3 {
        eprintln!("Usage: pydeps-cell <cell-dir> <python-deps.toml>");
        return ExitCode::from(1);
    }
    let (cell_dir, deps_path) = (Path::new(&args[1]), Path::new(&args[2]));
    // A Go panic exits 2: so does a panic here
    match std::panic::catch_unwind(|| run(cell_dir, deps_path)) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(err)) => {
            eprintln!("pydeps-cell: {err:#}");
            ExitCode::from(1)
        }
        Err(_) => ExitCode::from(2),
    }
}

fn run(cell_dir: &Path, deps_path: &Path) -> anyhow::Result<()> {
    let path = cell::join(cell_dir, &["pydeps-cell.json"]);
    let data = std::fs::read(&path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    let cfg: cell::Config = serde_json::from_str(&deps_gen_kit::gojson::text(&data))
        .map_err(|e| anyhow::anyhow!("parsing pydeps-cell.json: {e}"))?;
    let deps = cell::load_deps(deps_path)?;
    cell::render_cell(cell_dir, &deps, &cfg)
}
