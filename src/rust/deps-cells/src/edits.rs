//! Edits to deps cells' files, and the patches made from them
//!
//! `tk compose edit` copies a file of a deps cell to `.turnkey/edits/<cell>/`
//! for the developer to change, and `tk compose patch` diffs each edited
//! copy against the file in the Nix store into `.turnkey/patches/<cell>/`,
//! where the cell's fixups apply it.

use std::io;
use std::path::{Path, PathBuf};

use crate::index::{ResolveError, current_index};
use crate::{Error, cell_dir};
use gostd::path;
use gostd::strconv::quote;

/// Where edited copies are, relative to the project root
pub const EDITS_DIR: &str = ".turnkey/edits";
/// Where patches are, relative to the project root
pub const PATCHES_DIR: &str = ".turnkey/patches";

/// A deps cell's file, found in the Nix store
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellFile {
    /// The file in the Nix store
    pub store_file: String,
    /// The path patches name it by, in the cell
    pub patch_path: String,
    /// The package holding it (`vendor/anyhow@1.0.100`, or a Go module's
    /// `vendor/<module path>`); empty in a cell that is a symlink to one
    /// store path
    pub package: String,
}

/// Finds a file of a deps cell in the Nix store, and the path patches name
/// it by. A cell that is a symlink to one store path holds it under that
/// path. A materialized cell (ADR 0004) holds it in a package's own store
/// path: the cell index resolves `vendor/<pkg>/...`, and an alias package
/// (`vendor/anyhow`) to the package it forwards to
/// (`vendor/anyhow@1.0.100`), which is the path patches are routed by. In
/// the Go cell, the package is the module's path (`vendor/<module path>`),
/// whichever of its Go packages holds the file (ADR 0008); a path under a
/// forwarding alias package is a go.work member's code, edited in the
/// repo, not composed.
pub fn resolve_cell_file(root: &Path, cell: &str, rel_path: &str) -> Result<CellFile, Error> {
    let dir = cell_dir(root, cell);
    let info = std::fs::symlink_metadata(&dir)
        .map_err(|e| Error::new(format!("cell {} not found: {e}", quote(cell))))?;
    if info.file_type().is_symlink() {
        let target = std::fs::read_link(&dir)
            .map_err(|e| Error::new(format!("cell {}: {e}", quote(cell))))?;
        return Ok(CellFile {
            store_file: path::join(&[&target.to_string_lossy(), rel_path]),
            patch_path: rel_path.to_string(),
            package: String::new(),
        });
    }
    let index = current_index(root, cell)?;
    match index.resolve(rel_path) {
        Ok(resolved) => Ok(CellFile {
            store_file: path::join(&[&resolved.store, &resolved.rest]),
            patch_path: format!("{}/{}", resolved.root, resolved.rest),
            package: resolved.root,
        }),
        Err(e @ ResolveError::FirstParty { .. }) => Err(Error::new(format!(
            "{cell}/{rel_path} is the repo's own code, not the {cell} cell's: {e}; edit it in place"
        ))),
        Err(e) => Err(Error::new(e.to_string())),
    }
}

/// Where the patch of a cell's file goes: `<cell>/<path with / as ->.patch`
/// under the patches directory, or, for a materialized cell's file, in its
/// package's directory, which routes it to the package's own derivation
pub fn patch_file(patches_dir: &Path, cell: &str, file: &CellFile) -> PathBuf {
    if file.package.is_empty() {
        let name = format!("{}.patch", file.patch_path.replace('/', "-"));
        return patches_dir.join(cell).join(name);
    }
    let in_package = file
        .patch_path
        .strip_prefix(&format!("{}/", file.package))
        .unwrap_or(&file.patch_path);
    let name = format!("{}.patch", in_package.replace('/', "-"));
    let dir = path::join(&[&patches_dir.join(cell).to_string_lossy(), &file.package]);
    PathBuf::from(dir).join(name)
}

/// A cell's files under the edits or the patches directory
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellFiles {
    /// The cell
    pub cell: String,
    /// Its files, relative to its directory there
    pub files: Vec<String>,
}

/// Each cell's edited files, the cells and their files in name order
pub fn list_edited_files(edits_dir: &Path) -> Vec<CellFiles> {
    list_cell_files(edits_dir, |_| true)
}

/// Each cell's patches (`.patch` files, in package directories for a
/// materialized cell), the cells and their patches in name order
pub fn list_patch_files(patches_dir: &Path) -> Vec<CellFiles> {
    list_cell_files(patches_dir, |name| name.ends_with(".patch"))
}

fn list_cell_files(dir: &Path, wanted: impl Fn(&str) -> bool) -> Vec<CellFiles> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut cells: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    cells.sort();
    let mut result = Vec::new();
    for (cell, cell_path) in cells {
        // filepath.Walk: in name order, symlinks not followed, errors
        // skipped
        let files: Vec<String> = walkdir::WalkDir::new(&cell_path)
            .sort_by_file_name()
            .into_iter()
            .flatten()
            .filter(|e| !e.file_type().is_dir())
            .filter(|e| wanted(&e.file_name().to_string_lossy()))
            .filter_map(|e| {
                e.path()
                    .strip_prefix(&cell_path)
                    .ok()
                    .map(|rel| rel.to_string_lossy().into_owned())
            })
            .collect();
        if !files.is_empty() {
            result.push(CellFiles { cell, files });
        }
    }
    result
}

/// How many files the cells hold
pub fn count_files(cells: &[CellFiles]) -> usize {
    cells.iter().map(|c| c.files.len()).sum()
}

/// Copies a file, the copy written 0644
pub fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
    let data = std::fs::read(src)?;
    crate::fsutil::write_file(dst, &data)
}

/// Removes the empty directories from `dir` up, stopping at `stop_at`
pub fn clean_empty_dirs(dir: &Path, stop_at: &Path) {
    let stop_at = stop_at.to_string_lossy().into_owned();
    let mut dir = dir.to_string_lossy().into_owned();
    while dir != stop_at && dir != "." && dir != "/" {
        let empty = std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_none());
        if !empty {
            break;
        }
        let _ = std::fs::remove_dir(&dir);
        dir = path::dir(&dir);
    }
}

/// The unified diff of two files, labelled; empty when they hold the same
/// lines
pub fn generate_unified_diff(
    original: &Path,
    modified: &Path,
    original_label: &str,
    modified_label: &str,
) -> io::Result<Vec<u8>> {
    let original = std::fs::read(original)?;
    let modified = std::fs::read(modified)?;
    Ok(unified_diff(
        &lines(&original),
        &lines(&modified),
        original_label,
        modified_label,
    ))
}

/// A file's lines. The newline ending the last line ends it: it doesn't
/// start another, empty line, which would become a context line the file
/// doesn't have.
fn lines(data: &[u8]) -> Vec<&[u8]> {
    let text = data.strip_suffix(b"\n").unwrap_or(data);
    if text.is_empty() {
        return Vec::new();
    }
    text.split(|&b| b == b'\n').collect()
}

/// The unified diff of two sets of lines, labelled; empty when they are
/// the same
pub fn unified_diff(
    original: &[&[u8]],
    modified: &[&[u8]],
    original_label: &str,
    modified_label: &str,
) -> Vec<u8> {
    let lcs = longest_common_subsequence(original, modified);
    let hunks = build_hunks(original, modified, &lcs);
    if hunks.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend_from_slice(format!("--- {original_label}\n").as_bytes());
    out.extend_from_slice(format!("+++ {modified_label}\n").as_bytes());
    for hunk in hunks {
        out.extend_from_slice(
            format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.orig_start, hunk.orig_count, hunk.mod_start, hunk.mod_count
            )
            .as_bytes(),
        );
        for (kind, text) in hunk.lines {
            out.push(kind);
            out.extend_from_slice(text);
            out.push(b'\n');
        }
    }
    out
}

/// A hunk of a unified diff
struct Hunk<'a> {
    orig_start: usize,
    orig_count: usize,
    mod_start: usize,
    mod_count: usize,
    /// Each line, and its kind: ' ', '+' or '-'
    lines: Vec<(u8, &'a [u8])>,
}

/// The longest common subsequence of two sets of lines, as the pairs of
/// their indexes
fn longest_common_subsequence(a: &[&[u8]], b: &[&[u8]]) -> Vec<(usize, usize)> {
    let (m, n) = (a.len(), b.len());
    if m == 0 || n == 0 {
        return Vec::new();
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    let mut result = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            result.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] > dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    result.reverse();
    result
}

/// The diff's hunks, from the lines and their longest common subsequence
fn build_hunks<'a>(
    original: &[&'a [u8]],
    modified: &[&'a [u8]],
    lcs: &[(usize, usize)],
) -> Vec<Hunk<'a>> {
    const CONTEXT_LINES: usize = 3;
    let lcs_orig: std::collections::HashSet<usize> = lcs.iter().map(|p| p.0).collect();
    let lcs_mod: std::collections::HashSet<usize> = lcs.iter().map(|p| p.1).collect();

    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    let (mut orig_idx, mut mod_idx, mut lcs_idx) = (0, 0, 0);

    while orig_idx < original.len() || mod_idx < modified.len() {
        let in_lcs = lcs_idx < lcs.len() && (orig_idx, mod_idx) == lcs[lcs_idx];
        if in_lcs {
            // A common line: context
            if let Some(hunk) = current.as_mut() {
                hunk.lines.push((b' ', original[orig_idx]));
                hunk.orig_count += 1;
                hunk.mod_count += 1;
            }
            orig_idx += 1;
            mod_idx += 1;
            lcs_idx += 1;
        } else {
            // A difference: start or extend a hunk
            let hunk = current.get_or_insert_with(|| {
                let context_start = orig_idx.saturating_sub(CONTEXT_LINES);
                let mod_context_start = mod_idx.saturating_sub(CONTEXT_LINES);
                let mut hunk = Hunk {
                    orig_start: context_start + 1,
                    orig_count: 0,
                    mod_start: mod_context_start + 1,
                    mod_count: 0,
                    lines: Vec::new(),
                };
                for line in &original[context_start..orig_idx] {
                    hunk.lines.push((b' ', line));
                    hunk.orig_count += 1;
                    hunk.mod_count += 1;
                }
                hunk
            });
            while orig_idx < original.len() && !lcs_orig.contains(&orig_idx) {
                hunk.lines.push((b'-', original[orig_idx]));
                hunk.orig_count += 1;
                orig_idx += 1;
            }
            while mod_idx < modified.len() && !lcs_mod.contains(&mod_idx) {
                hunk.lines.push((b'+', modified[mod_idx]));
                hunk.mod_count += 1;
                mod_idx += 1;
            }
        }

        // Once the common lines are all used, the hunk ends
        if lcs_idx >= lcs.len()
            && let Some(hunk) = current.take()
            && hunk.lines.iter().any(|(kind, _)| *kind != b' ')
        {
            hunks.push(hunk);
        }
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(original: &str, modified: &str) -> String {
        let (o, m) = (original.as_bytes(), modified.as_bytes());
        String::from_utf8(unified_diff(&lines(o), &lines(m), "a/f", "b/f")).unwrap()
    }

    #[test]
    fn same_lines_no_diff() {
        assert_eq!(diff("a\nb\n", "a\nb\n"), "");
        // Only the last newline differs: the lines are the same
        assert_eq!(diff("a\nb", "a\nb\n"), "");
        assert_eq!(diff("", ""), "");
    }

    #[test]
    fn appending_to_the_last_line_adds_no_context_line() {
        let body = "fn a() {\n    }\n}\nfn b() {\n    }\n}\n";
        let got = diff(body, &format!("{body}// appended\n"));
        assert_eq!(
            got,
            "--- a/f\n+++ b/f\n@@ -4,3 +4,4 @@\n fn b() {\n     }\n }\n+// appended\n"
        );
        assert!(!got.contains("\n \n"));
    }

    #[test]
    fn a_hunk_runs_from_the_first_change_to_the_last_common_line() {
        let original = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let modified = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n10\n";
        assert_eq!(
            diff(original, modified),
            "--- a/f\n+++ b/f\n@@ -2,9 +2,9 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n 9\n 10\n"
        );
    }

    #[test]
    fn changes_after_the_last_common_line_get_a_hunk_of_their_own() {
        assert_eq!(
            diff("a\nb\nc\n", "x\na\nb\nc\ny\n"),
            "--- a/f\n+++ b/f\n@@ -1,3 +1,4 @@\n+x\n a\n b\n c\n@@ -1,3 +2,4 @@\n a\n b\n c\n+y\n"
        );
    }

    #[test]
    fn removals_and_an_empty_original() {
        assert_eq!(
            diff("a\nb\nc\n", "a\nc\n"),
            "--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n a\n-b\n c\n"
        );
        assert_eq!(diff("", "x\n"), "--- a/f\n+++ b/f\n@@ -1,0 +1,1 @@\n+x\n");
        assert_eq!(diff("x\n", ""), "--- a/f\n+++ b/f\n@@ -1,1 +1,0 @@\n-x\n");
    }

    #[test]
    fn lines_are_bytes() {
        let original: &[&[u8]] = &[b"\xff", b"same"];
        let modified: &[&[u8]] = &[b"\xfe", b"same"];
        assert_eq!(
            unified_diff(original, modified, "a/f", "b/f"),
            b"--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-\xff\n+\xfe\n same\n".to_vec()
        );
    }

    /// A patch from tk compose applies with no fuzz, at the line it was
    /// made for
    #[test]
    fn generated_patch_applies_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let body = "fn a() {\n    }\n}\nfn b() {\n    }\n}\n";
        let original = dir.path().join("a.rs");
        let edited = dir.path().join("b.rs");
        std::fs::write(&original, body).unwrap();
        std::fs::write(&edited, format!("{body}// appended\n")).unwrap();
        let patch = generate_unified_diff(&original, &edited, "a/lib.rs", "b/lib.rs").unwrap();

        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("lib.rs"), body).unwrap();
        let child = std::process::Command::new("patch")
            .args(["-p1", "--fuzz=0", "--forward"])
            .current_dir(&target)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            eprintln!("no patch command to apply it with");
            return;
        };
        use std::io::Write as _;
        child.stdin.take().unwrap().write_all(&patch).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "patch: {out:?}");
        assert_eq!(
            std::fs::read_to_string(target.join("lib.rs")).unwrap(),
            format!("{body}// appended\n")
        );
    }

    #[test]
    fn edits_and_patches_are_listed_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "edits/rustdeps/vendor/b/lib.rs",
            "edits/rustdeps/vendor/a/lib.rs",
            "edits/godeps/vendor/x/x.go",
            "edits/pydeps/vendor/p/p.py",
            "edits/not-a-cell",
            "patches/rustdeps/vendor/b/lib.rs.patch",
            "patches/godeps/vendor-x-x.go.patch",
            "patches/godeps/notes.txt",
        ] {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "").unwrap();
        }
        let cells = |entries: &[(&str, &[&str])]| -> Vec<CellFiles> {
            entries
                .iter()
                .map(|(cell, files)| CellFiles {
                    cell: cell.to_string(),
                    files: files.iter().map(|f| f.to_string()).collect(),
                })
                .collect()
        };

        let edited = list_edited_files(&dir.path().join("edits"));
        assert_eq!(
            edited,
            cells(&[
                ("godeps", &["vendor/x/x.go"]),
                ("pydeps", &["vendor/p/p.py"]),
                ("rustdeps", &["vendor/a/lib.rs", "vendor/b/lib.rs"]),
            ])
        );
        assert_eq!(count_files(&edited), 4);
        assert_eq!(
            list_patch_files(&dir.path().join("patches")),
            cells(&[
                ("godeps", &["vendor-x-x.go.patch"]),
                ("rustdeps", &["vendor/b/lib.rs.patch"]),
            ])
        );
        assert!(list_edited_files(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn patch_files() {
        let patches = Path::new("/p/.turnkey/patches");
        let symlinked = CellFile {
            store_file: String::new(),
            patch_path: "vendor/github.com/spf13/cobra/command.go".into(),
            package: String::new(),
        };
        assert_eq!(
            patch_file(patches, "godeps", &symlinked),
            Path::new("/p/.turnkey/patches/godeps/vendor-github.com-spf13-cobra-command.go.patch")
        );
        let materialized = CellFile {
            store_file: String::new(),
            patch_path: "vendor/anyhow@1.0.100/src/lib.rs".into(),
            package: "vendor/anyhow@1.0.100".into(),
        };
        assert_eq!(
            patch_file(patches, "rustdeps", &materialized),
            Path::new("/p/.turnkey/patches/rustdeps/vendor/anyhow@1.0.100/src-lib.rs.patch")
        );
    }

    #[test]
    fn cleans_empty_dirs_up_to_the_stop() {
        let dir = tempfile::tempdir().unwrap();
        let edits = dir.path().join("edits");
        std::fs::create_dir_all(edits.join("c/a/b")).unwrap();
        std::fs::write(edits.join("c/keep"), "").unwrap();
        clean_empty_dirs(&edits.join("c/a/b"), &edits);
        assert!(!edits.join("c/a").exists());
        assert!(edits.join("c").exists());
    }
}
