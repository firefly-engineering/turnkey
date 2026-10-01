//! pydeps-cell writes the rules.star files of the pydeps cell's vendored
//! packages, with the dependencies between them evaluated per platform.
//!
//! It runs inside the Python deps-cell derivation
//! (nix/lib/deps-cell/adapters/python.nix), once the packages are merged
//! into the cell.
//!
//! It is the Rust port of src/cmd/pydeps-cell and src/go/pkg/pydepscell
//! (#212), and writes the same bytes.
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
    let cfg: cell::Config = serde_json::from_str(&go_json_text(&data))
        .map_err(|e| anyhow::anyhow!("parsing pydeps-cell.json: {e}"))?;
    let deps = cell::load_deps(deps_path)?;
    cell::render_cell(cell_dir, &deps, &cfg)
}

/// JSON input as encoding/json reads it: each byte that isn't part of a
/// valid UTF-8 sequence is U+FFFD (in a string; anywhere else it is a
/// syntax error either way)
fn go_json_text(mut data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    loop {
        match std::str::from_utf8(data) {
            Ok(rest) => {
                out.push_str(rest);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&data[..valid]).expect("valid up to here"));
                out.push('\u{fffd}');
                data = &data[valid + 1..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_utf8_is_replaced_byte_by_byte() {
        assert_eq!(
            go_json_text(b"a\xe2\x82b\xffc"),
            "a\u{fffd}\u{fffd}b\u{fffd}c"
        );
        assert_eq!(go_json_text("été".as_bytes()), "été");
    }
}
