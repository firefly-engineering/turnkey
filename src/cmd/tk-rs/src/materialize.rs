//! `tk materialize`: deps cells brought in line with their cell indexes
//! (ADR 0004). The shell calls it with the indexes it built, where it keeps
//! its other links current.

use std::io::Write;
use std::path::Path;

use deps_cells::{AddRoot, Options, materialize, stale};

/// Materializes the cell of each index in `args` under `root`, rooting
/// each index with `add_root`, and returns the exit code: 1 if any failed.
/// `verbose` and `quiet` are tk's.
pub fn run(
    args: &[String],
    root: &Path,
    add_root: AddRoot,
    verbose: bool,
    quiet: bool,
    stderr: &mut dyn Write,
) -> i32 {
    let mut status = 0;
    for index in args {
        let outcome = match materialize(Options {
            root: root.to_path_buf(),
            index_path: index.clone(),
            add_root: Some(&mut *add_root),
        }) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = writeln!(stderr, "tk: materializing {index}: {e}");
                status = 1;
                continue;
            }
        };
        if outcome.switched_over && !quiet {
            let _ = writeln!(
                stderr,
                "tk: the deps cell for {index} is now a real directory (to go back: rm -rf the cell)"
            );
        }
        if (verbose || (!quiet && outcome.changed())) && !outcome.switched_over {
            let _ = writeln!(
                stderr,
                "tk: materialized {index}: {} store links added, {} removed; {} packages written, {} removed",
                outcome.store_links_added,
                outcome.store_links_gone,
                outcome.packages_written,
                outcome.packages_gone
            );
        }
    }
    status
}

/// The usage `tk materialize` exits 1 with when given no index
pub const USAGE: &str = "Usage: tk materialize <cell index>...";

/// Warns when a materialized deps cell was built from another version of
/// its deps file than the one on disk: the shell hasn't re-evaluated since
/// the file changed. `cells` are the sync config's languages' cells and
/// deps files; those missing either are skipped.
pub fn warn_stale_cells<'a>(
    root: &Path,
    cells: impl IntoIterator<Item = (&'a str, &'a str)>,
    quiet: bool,
    stderr: &mut dyn Write,
) {
    for (cell, deps_file) in cells {
        if cell.is_empty() || deps_file.is_empty() {
            continue;
        }
        if stale(root, cell, deps_file).unwrap_or(false) && !quiet {
            let _ = writeln!(
                stderr,
                "tk: warning: the {cell} cell was built from another {deps_file}; reload the shell (direnv reload) to rebuild it"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A project and a store holding the index of a cell with one package
    fn fixture() -> (tempfile::TempDir, PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let store = dir.path().join("store");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(store.join("aaa-dep")).unwrap();
        let index = store.join("index.json");
        std::fs::write(
            &index,
            format!(
                r#"{{"cell": "rustdeps", "deps_file_sha256": "abc", "buckconfig": "", "packages": {{"vendor/a@1": {{"store": "{}", "targets": ["a"]}}}}, "aliases": {{}}}}"#,
                store.join("aaa-dep").display()
            ),
        )
        .unwrap();
        (dir, root, index.to_string_lossy().into_owned())
    }

    fn run_with(root: &Path, args: &[String], verbose: bool, quiet: bool) -> (i32, String, usize) {
        let mut stderr = Vec::new();
        let mut rooted = 0;
        let mut add_root = |link: &Path, store: &str| -> Result<(), String> {
            rooted += 1;
            std::os::unix::fs::symlink(store, link).map_err(|e| e.to_string())
        };
        let code = run(args, root, &mut add_root, verbose, quiet, &mut stderr);
        (code, String::from_utf8(stderr).unwrap(), rooted)
    }

    #[test]
    fn reports_what_changed() {
        let (_dir, root, index) = fixture();
        let (code, stderr, rooted) = run_with(&root, std::slice::from_ref(&index), false, false);
        assert_eq!((code, rooted), (0, 1));
        assert_eq!(
            stderr,
            format!(
                "tk: materialized {index}: 1 store links added, 0 removed; 1 packages written, 0 removed\n"
            )
        );
        // A no-op says nothing, unless verbose
        let (_, stderr, _) = run_with(&root, std::slice::from_ref(&index), false, false);
        assert_eq!(stderr, "");
        let (_, stderr, _) = run_with(&root, std::slice::from_ref(&index), true, false);
        assert!(stderr.contains("0 store links added"), "{stderr}");
    }

    #[test]
    fn reports_a_switch_over_unless_quiet() {
        let (_dir, root, index) = fixture();
        std::fs::create_dir_all(root.join(".turnkey")).unwrap();
        std::os::unix::fs::symlink("/nix/store/old", root.join(".turnkey/rustdeps")).unwrap();
        let (code, stderr, _) = run_with(&root, std::slice::from_ref(&index), true, false);
        assert_eq!(code, 0);
        assert_eq!(
            stderr,
            format!(
                "tk: the deps cell for {index} is now a real directory (to go back: rm -rf the cell)\n"
            )
        );
    }

    #[test]
    fn goes_on_after_a_failure_and_exits_1() {
        let (_dir, root, index) = fixture();
        let args = ["/nonexistent/index.json".to_string(), index.clone()];
        let (code, stderr, rooted) = run_with(&root, &args, false, true);
        assert_eq!((code, rooted), (1, 1));
        assert!(
            stderr.starts_with("tk: materializing /nonexistent/index.json: "),
            "{stderr}"
        );
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }

    #[test]
    fn warns_of_stale_cells() {
        let (_dir, root, index) = fixture();
        run_with(&root, &[index], false, true);
        std::fs::write(root.join("rust-deps.toml"), "changed").unwrap();
        let mut stderr = Vec::new();
        let cells = [
            ("rustdeps", "rust-deps.toml"),
            ("", "x.toml"),
            ("godeps", ""),
        ];
        warn_stale_cells(&root, cells, false, &mut stderr);
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "tk: warning: the rustdeps cell was built from another rust-deps.toml; reload the shell (direnv reload) to rebuild it\n"
        );
        let mut stderr = Vec::new();
        warn_stale_cells(&root, cells, true, &mut stderr);
        assert!(stderr.is_empty());
    }
}
