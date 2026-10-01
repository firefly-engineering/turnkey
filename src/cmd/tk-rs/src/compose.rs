//! `tk compose`: editing deps cells' files, and making patches of the
//! edits
//!
//! ```text
//! tk compose status              # Show edited files
//! tk compose edit <cell/path>    # Copy file from cell to edits for modification
//! tk compose patch               # Generate patches from edited files
//! tk compose patch <cell>        # Generate patches for specific cell
//! tk compose reset               # Revert all edits
//! tk compose reset <cell>        # Revert edits for specific cell
//! tk compose reset <cell/path>   # Revert specific file edit
//! ```

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::help::COMPOSE_HELP;
use deps_cells::edits::{
    self, CellFiles, EDITS_DIR, PATCHES_DIR, list_edited_files, list_patch_files, resolve_cell_file,
};
use gostd::strconv::quote;

/// Finds the project root, or says why it can't
pub type FindRoot<'a> = &'a dyn Fn() -> Result<PathBuf, String>;

/// What `tk compose` reads and writes
pub struct Io<'a> {
    /// Where results go
    pub stdout: &'a mut dyn Write,
    /// Where errors and help go
    pub stderr: &'a mut dyn Write,
    /// Where `reset` reads its confirmation from
    pub stdin: &'a mut dyn BufRead,
}

/// Runs `tk compose` with `args`, and returns its exit code. `verbose` is
/// tk's `--verbose`.
pub fn run(args: &[String], find_root: FindRoot, verbose: bool, io: &mut Io) -> i32 {
    let Some(subcommand) = args.first() else {
        let _ = writeln!(io.stderr, "{COMPOSE_HELP}");
        return 0;
    };
    let rest = &args[1..];
    match subcommand.as_str() {
        "status" => status(rest, find_root, io),
        "edit" => edit(rest, find_root, io),
        "patch" => patch(rest, find_root, verbose, io),
        "reset" => reset(rest, find_root, io),
        "help" | "--help" | "-h" => {
            let _ = writeln!(io.stderr, "{COMPOSE_HELP}");
            0
        }
        other => {
            let _ = writeln!(io.stderr, "tk compose: unknown subcommand {}", quote(other));
            let _ = writeln!(io.stderr, "{COMPOSE_HELP}");
            1
        }
    }
}

/// The project root, or the error tk compose exits 1 with
fn root(find_root: FindRoot, io: &mut Io) -> Option<PathBuf> {
    match find_root() {
        Ok(root) => Some(root),
        Err(e) => {
            let _ = writeln!(io.stderr, "tk compose: {e}");
            None
        }
    }
}

/// Prints each cell's files, under a title
fn print_cells(out: &mut dyn Write, cells: &[CellFiles]) {
    for c in cells {
        let _ = writeln!(out, "\n  {}:", c.cell);
        for f in &c.files {
            let _ = writeln!(out, "    {f}");
        }
    }
}

/// `tk compose status`: the edited files, and with `--patches` the
/// generated patches
fn status(args: &[String], find_root: FindRoot, io: &mut Io) -> i32 {
    let Some(root) = root(find_root, io) else {
        return 1;
    };
    let show_patches = args.iter().any(|a| a == "--patches" || a == "-p");

    let edited = list_edited_files(&root.join(EDITS_DIR));
    if edited.is_empty() {
        let _ = writeln!(io.stdout, "No edited files.");
    } else {
        let _ = writeln!(io.stdout, "Edited files ({}):", edited.len());
        print_cells(io.stdout, &edited);
    }

    if show_patches {
        let patches = list_patch_files(&root.join(PATCHES_DIR));
        if patches.is_empty() {
            let _ = writeln!(io.stdout, "\nNo patches generated.");
        } else {
            let _ = writeln!(io.stdout, "\nGenerated patches ({}):", patches.len());
            print_cells(io.stdout, &patches);
        }
    }
    0
}

/// `tk compose edit <cell>/<path>`: copies a file of a cell to the edits
/// directory, to be changed
fn edit(args: &[String], find_root: FindRoot, io: &mut Io) -> i32 {
    let Some(target) = args.first() else {
        let _ = writeln!(io.stderr, "tk compose edit: missing argument");
        let _ = writeln!(io.stderr, "Usage: tk compose edit <cell>/<path>");
        let _ = writeln!(
            io.stderr,
            "Example: tk compose edit godeps/vendor/github.com/spf13/cobra/command.go"
        );
        return 1;
    };
    let Some(root) = root(find_root, io) else {
        return 1;
    };
    let Some((cell, rel_path)) = target.split_once('/') else {
        let _ = writeln!(
            io.stderr,
            "tk compose edit: invalid path format, expected <cell>/<path>"
        );
        return 1;
    };

    // The file in the Nix store, and its path in the cell as patches name
    // it
    let file = match resolve_cell_file(&root, cell, rel_path) {
        Ok(file) => file,
        Err(e) => {
            let _ = writeln!(io.stderr, "tk compose edit: {e}");
            return 1;
        }
    };
    if std::fs::metadata(&file.store_file).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        let _ = writeln!(
            io.stderr,
            "tk compose edit: file not found: {}",
            file.store_file
        );
        return 1;
    }

    let edit_file = join(&root.join(EDITS_DIR).join(cell), &file.patch_path);
    if std::fs::metadata(&edit_file).is_ok() {
        let _ = writeln!(
            io.stderr,
            "tk compose edit: file already being edited: {}",
            edit_file.display()
        );
        let _ = writeln!(
            io.stderr,
            "Use 'tk compose reset {target}' to revert first."
        );
        return 1;
    }
    if let Some(parent) = edit_file.parent()
        && let Err(e) = mkdir_all(parent)
    {
        let _ = writeln!(
            io.stderr,
            "tk compose edit: failed to create directory: {e}"
        );
        return 1;
    }
    if let Err(e) = edits::copy_file(Path::new(&file.store_file), &edit_file) {
        let _ = writeln!(io.stderr, "tk compose edit: failed to copy file: {e}");
        return 1;
    }

    let _ = writeln!(io.stdout, "Created editable copy: {}", edit_file.display());
    let _ = writeln!(io.stdout, "Original: {}", file.store_file);
    let _ = writeln!(
        io.stdout,
        "\nEdit the file, then run 'tk compose patch' to generate a patch."
    );
    0
}

/// `tk compose patch [cell]`: a patch of each edited file
fn patch(args: &[String], find_root: FindRoot, mut verbose: bool, io: &mut Io) -> i32 {
    let Some(root) = root(find_root, io) else {
        return 1;
    };
    // The last argument that isn't a flag filters the cells
    let mut cell_filter = "";
    for arg in args {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            arg if !arg.is_empty() && !arg.starts_with('-') => cell_filter = arg,
            _ => {}
        }
    }

    let edits_path = root.join(EDITS_DIR);
    let patches_path = root.join(PATCHES_DIR);
    let edited = list_edited_files(&edits_path);
    if edited.is_empty() {
        let _ = writeln!(io.stdout, "No edited files to generate patches from.");
        return 0;
    }

    let mut patch_count = 0;
    for CellFiles { cell, files } in &edited {
        if !cell_filter.is_empty() && cell != cell_filter {
            continue;
        }
        for edit_path in files {
            let edited_file = edits_path.join(cell).join(edit_path);
            let file = match resolve_cell_file(&root, cell, edit_path) {
                Ok(file) => file,
                Err(e) => {
                    let _ = writeln!(io.stderr, "tk compose patch: {e}");
                    continue;
                }
            };
            let rel_path = &file.patch_path;
            // A materialized cell's patches go in their package's
            // directory, which routes each to the package's own derivation
            let patch_file = edits::patch_file(&patches_path, cell, &file);

            let diff = match edits::generate_unified_diff(
                Path::new(&file.store_file),
                &edited_file,
                &format!("a/{rel_path}"),
                &format!("b/{rel_path}"),
            ) {
                Ok(diff) => diff,
                Err(e) => {
                    let _ = writeln!(
                        io.stderr,
                        "tk compose patch: failed to diff {rel_path}: {e}"
                    );
                    continue;
                }
            };
            if diff.is_empty() {
                if verbose {
                    let _ = writeln!(io.stdout, "No changes: {cell}/{rel_path}");
                }
                continue;
            }

            if let Some(parent) = patch_file.parent()
                && let Err(e) = mkdir_all(parent)
            {
                let _ = writeln!(
                    io.stderr,
                    "tk compose patch: failed to create directory: {e}"
                );
                continue;
            }
            if let Err(e) = deps_cells::write_file(&patch_file, &diff) {
                let _ = writeln!(io.stderr, "tk compose patch: failed to write patch: {e}");
                continue;
            }
            let _ = writeln!(io.stdout, "Generated: {}", patch_file.display());
            patch_count += 1;
        }
    }

    if patch_count == 0 {
        let _ = writeln!(io.stdout, "No patches generated (no changes detected).");
    } else {
        let _ = writeln!(io.stdout, "\nGenerated {patch_count} patch(es).");
    }
    0
}

/// `tk compose reset [cell[/path]]`: reverts edits, all of them or a
/// cell's after a confirmation (unless `--force`), or one file's
fn reset(args: &[String], find_root: FindRoot, io: &mut Io) -> i32 {
    let Some(root) = root(find_root, io) else {
        return 1;
    };
    // The last argument that isn't a flag is what to reset
    let mut force = false;
    let mut target = "";
    for arg in args {
        match arg.as_str() {
            "--force" | "-f" => force = true,
            "--verbose" | "-v" => {}
            arg if !arg.is_empty() && !arg.starts_with('-') => target = arg,
            _ => {}
        }
    }

    let edits_path = root.join(EDITS_DIR);

    if target.is_empty() {
        let edited = list_edited_files(&edits_path);
        if edited.is_empty() {
            let _ = writeln!(io.stdout, "No edited files to reset.");
            return 0;
        }
        let question = format!(
            "This will remove all {} edited file(s). Continue? [y/N] ",
            edits::count_files(&edited)
        );
        if !force && !confirm(&question, io) {
            let _ = writeln!(io.stdout, "Aborted.");
            return 0;
        }
        if let Err(e) = remove_all(&edits_path) {
            let _ = writeln!(io.stderr, "tk compose reset: failed to remove edits: {e}");
            return 1;
        }
        let _ = writeln!(io.stdout, "All edits have been reverted.");
        return 0;
    }

    let Some((cell, rel_path)) = target.split_once('/') else {
        // A whole cell
        let cell = target;
        let cell_edits = join(&edits_path, cell);
        if std::fs::metadata(&cell_edits).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            let _ = writeln!(io.stdout, "No edits for cell {}.", quote(cell));
            return 0;
        }
        let question = format!(
            "This will remove all edits for cell {}. Continue? [y/N] ",
            quote(cell)
        );
        if !force && !confirm(&question, io) {
            let _ = writeln!(io.stdout, "Aborted.");
            return 0;
        }
        if let Err(e) = remove_all(&cell_edits) {
            let _ = writeln!(io.stderr, "tk compose reset: failed to remove edits: {e}");
            return 1;
        }
        let _ = writeln!(io.stdout, "Reverted all edits for cell {}.", quote(cell));
        return 0;
    };

    // One file, named as edit named it (an alias package's path resolves
    // to its package's)
    let rel_path = match resolve_cell_file(&root, cell, rel_path) {
        Ok(file) => file.patch_path,
        Err(_) => rel_path.to_string(),
    };
    let edit_file = join(&edits_path.join(cell), &rel_path);
    if std::fs::metadata(&edit_file).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        let _ = writeln!(io.stdout, "File not being edited: {target}");
        return 0;
    }
    if let Err(e) = std::fs::remove_file(&edit_file) {
        let _ = writeln!(io.stderr, "tk compose reset: failed to remove file: {e}");
        return 1;
    }
    if let Some(parent) = edit_file.parent() {
        edits::clean_empty_dirs(parent, &edits_path);
    }
    let _ = writeln!(io.stdout, "Reverted: {target}");
    0
}

/// Asks `question` on stdout and reads the answer: whether it is yes
fn confirm(question: &str, io: &mut Io) -> bool {
    let _ = write!(io.stdout, "{question}");
    let _ = io.stdout.flush();
    let mut response = String::new();
    let _ = io.stdin.read_line(&mut response);
    let response = response.trim().to_lowercase();
    response == "y" || response == "yes"
}

/// `filepath.Join(base, rel)`: `rel` appended to `base` and cleaned, an
/// absolute `rel` included
fn join(base: &Path, rel: &str) -> PathBuf {
    PathBuf::from(gostd::path::join(&[&base.to_string_lossy(), rel]))
}

/// `os.MkdirAll(path, 0o755)`
fn mkdir_all(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(path)
}

/// `os.RemoveAll`: a missing path is no error
fn remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A project whose godeps cell is a symlink to a store path holding
    /// vendor/x/x.go, and whose rustdeps cell is materialized, with anyhow
    /// at 1.0.100 aliased as vendor/anyhow
    struct Project {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Project {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("project");
            let store = dir.path().join("store");
            let godeps = store.join("godeps-cell");
            std::fs::create_dir_all(godeps.join("vendor/x")).unwrap();
            std::fs::write(godeps.join("vendor/x/x.go"), "package x\n\nfunc X() {}\n").unwrap();
            std::fs::create_dir_all(root.join(".turnkey")).unwrap();
            symlink(&godeps, root.join(".turnkey/godeps")).unwrap();

            let anyhow = store.join("aaa-dep-rust-anyhow-1.0.100");
            std::fs::create_dir_all(anyhow.join("src")).unwrap();
            std::fs::write(anyhow.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
            let index = store.join("index.json");
            std::fs::write(
                &index,
                format!(
                    r#"{{"cell": "rustdeps", "packages": {{"vendor/anyhow@1.0.100": {{"store": "{}", "targets": ["anyhow"]}}}}, "aliases": {{"vendor/anyhow": "vendor/anyhow@1.0.100"}}}}"#,
                    anyhow.display()
                ),
            )
            .unwrap();
            let mut add_root = |link: &Path, store: &str| -> Result<(), String> {
                symlink(store, link).map_err(|e| e.to_string())
            };
            deps_cells::materialize(deps_cells::Options {
                root: root.clone(),
                index_path: index.to_string_lossy().into_owned(),
                add_root: Some(&mut add_root),
            })
            .unwrap();
            Project { _dir: dir, root }
        }

        /// Runs tk compose with `args` and `stdin`: its exit code, stdout
        /// and stderr
        fn compose(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            let mut stdin = stdin.as_bytes();
            let root = self.root.clone();
            let code = run(
                &args,
                &move || Ok(root.clone()),
                false,
                &mut Io {
                    stdout: &mut stdout,
                    stderr: &mut stderr,
                    stdin: &mut stdin,
                },
            );
            (
                code,
                String::from_utf8(stdout).unwrap(),
                String::from_utf8(stderr).unwrap(),
            )
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }
    }

    #[test]
    fn help_and_unknown_subcommands() {
        let p = Project::new();
        let (code, _, stderr) = p.compose(&[], "");
        assert_eq!(code, 0);
        assert!(stderr.starts_with("Usage: tk compose"), "{stderr}");
        let (code, _, stderr) = p.compose(&["frob"], "");
        assert_eq!(code, 1);
        assert!(
            stderr.starts_with("tk compose: unknown subcommand \"frob\"\nUsage"),
            "{stderr}"
        );
    }

    #[test]
    fn edit_patch_and_reset_a_symlinked_cells_file() {
        let p = Project::new();
        let (code, stdout, _) = p.compose(&["edit", "godeps/vendor/x/x.go"], "");
        assert_eq!(code, 0);
        let edited = p.path(".turnkey/edits/godeps/vendor/x/x.go");
        assert!(
            stdout.starts_with(&format!("Created editable copy: {}\n", edited.display())),
            "{stdout}"
        );
        let (code, _, stderr) = p.compose(&["edit", "godeps/vendor/x/x.go"], "");
        assert_eq!(code, 1);
        assert!(stderr.contains("already being edited"), "{stderr}");

        // Unchanged: no patch
        let (code, stdout, _) = p.compose(&["patch", "-v"], "");
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            "No changes: godeps/vendor/x/x.go\nNo patches generated (no changes detected).\n"
        );

        std::fs::write(&edited, "package x\n\nfunc X() { panic(1) }\n").unwrap();
        let (code, stdout, _) = p.compose(&["patch"], "");
        assert_eq!(code, 0);
        let patch = p.path(".turnkey/patches/godeps/vendor-x-x.go.patch");
        assert_eq!(
            stdout,
            format!("Generated: {}\n\nGenerated 1 patch(es).\n", patch.display())
        );
        assert_eq!(
            std::fs::read_to_string(&patch).unwrap(),
            "--- a/vendor/x/x.go\n+++ b/vendor/x/x.go\n@@ -1,3 +1,3 @@\n package x\n \n-func X() {}\n+func X() { panic(1) }\n"
        );

        let (_, stdout, _) = p.compose(&["status", "--patches"], "");
        assert_eq!(
            stdout,
            "Edited files (1):\n\n  godeps:\n    vendor/x/x.go\n\nGenerated patches (1):\n\n  godeps:\n    vendor-x-x.go.patch\n"
        );

        let (code, stdout, _) = p.compose(&["reset", "godeps/vendor/x/x.go"], "");
        assert_eq!(
            (code, stdout.as_str()),
            (0, "Reverted: godeps/vendor/x/x.go\n")
        );
        assert!(
            !p.path(".turnkey/edits/godeps").exists(),
            "empty directories left"
        );
        assert!(p.path(".turnkey/edits").exists());
        let (_, stdout, _) = p.compose(&["reset", "godeps/vendor/x/x.go"], "");
        assert_eq!(stdout, "File not being edited: godeps/vendor/x/x.go\n");
    }

    #[test]
    fn a_materialized_cells_file_is_named_by_its_package() {
        let p = Project::new();
        // Through the alias package
        let (code, _, stderr) = p.compose(&["edit", "rustdeps/vendor/anyhow/src/lib.rs"], "");
        assert_eq!(code, 0, "{stderr}");
        let edited = p.path(".turnkey/edits/rustdeps/vendor/anyhow@1.0.100/src/lib.rs");
        std::fs::write(&edited, "pub fn a() {}\npub fn b() {}\n").unwrap();

        let (_, stdout, _) = p.compose(&["patch", "rustdeps"], "");
        let patch = p.path(".turnkey/patches/rustdeps/vendor/anyhow@1.0.100/src-lib.rs.patch");
        assert!(
            stdout.starts_with(&format!("Generated: {}\n", patch.display())),
            "{stdout}"
        );
        assert!(std::fs::read_to_string(&patch).unwrap().starts_with(
            "--- a/vendor/anyhow@1.0.100/src/lib.rs\n+++ b/vendor/anyhow@1.0.100/src/lib.rs\n"
        ));
        // Another cell's filter: nothing
        let (_, stdout, _) = p.compose(&["patch", "godeps"], "");
        assert_eq!(stdout, "No patches generated (no changes detected).\n");

        let (_, stdout, _) = p.compose(&["reset", "rustdeps/vendor/anyhow/src/lib.rs"], "");
        assert_eq!(stdout, "Reverted: rustdeps/vendor/anyhow/src/lib.rs\n");
        assert!(!edited.exists());
    }

    #[test]
    fn reset_asks_first_unless_forced() {
        let p = Project::new();
        p.compose(&["edit", "godeps/vendor/x/x.go"], "");
        p.compose(&["edit", "rustdeps/vendor/anyhow/src/lib.rs"], "");

        let (code, stdout, _) = p.compose(&["reset"], "n\n");
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            "This will remove all 2 edited file(s). Continue? [y/N] Aborted.\n"
        );
        let (_, stdout, _) = p.compose(&["reset", "godeps"], " YES \n");
        assert_eq!(
            stdout,
            "This will remove all edits for cell \"godeps\". Continue? [y/N] Reverted all edits for cell \"godeps\".\n"
        );
        let (_, stdout, _) = p.compose(&["reset", "godeps", "-f"], "");
        assert_eq!(stdout, "No edits for cell \"godeps\".\n");
        let (_, stdout, _) = p.compose(&["reset", "--force"], "");
        assert_eq!(stdout, "All edits have been reverted.\n");
        assert!(!p.path(".turnkey/edits").exists());
        let (_, stdout, _) = p.compose(&["reset"], "");
        assert_eq!(stdout, "No edited files to reset.\n");
    }

    #[test]
    fn edit_errors() {
        let p = Project::new();
        for (args, want) in [
            (&["edit"][..], "tk compose edit: missing argument\n"),
            (&["edit", "godeps"], "tk compose edit: invalid path format"),
            (
                &["edit", "nocell/x"],
                "tk compose edit: cell \"nocell\" not found",
            ),
            (
                &["edit", "godeps/vendor/missing.go"],
                "tk compose edit: file not found: ",
            ),
            (
                &["edit", "rustdeps/vendor/serde/lib.rs"],
                "tk compose edit: vendor/serde/lib.rs is in no package of the rustdeps cell",
            ),
        ] {
            let (code, _, stderr) = p.compose(args, "");
            assert_eq!(code, 1, "{args:?}");
            assert!(stderr.starts_with(want), "{args:?}: {stderr}");
        }
    }
}
