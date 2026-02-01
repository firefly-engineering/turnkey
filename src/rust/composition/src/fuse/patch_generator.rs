//! Patch generation for edited external dependencies
//!
//! This module generates unified diff patches from files that have been
//! modified via the edit overlay. Patches are stored in `.turnkey/patches/`
//! and can be applied by Nix fixups during dependency builds.
//!
//! # Patch Format
//!
//! Patches are in unified diff format, compatible with the `patch` command:
//!
//! ```text
//! --- a/vendor/github.com/spf13/cobra/command.go
//! +++ b/vendor/github.com/spf13/cobra/command.go
//! @@ -10,7 +10,7 @@
//!  context line
//! -old line
//! +new line
//!  context line
//! ```
//!
//! # Patch Naming
//!
//! Patches are named based on the file path with `/` replaced by `-`:
//! - `vendor/github.com/spf13/cobra/command.go` → `vendor-github.com-spf13-cobra-command.go.patch`
//!
//! Patches are organized by cell:
//! ```text
//! .turnkey/patches/
//! ├── godeps/
//! │   └── vendor-github.com-spf13-cobra-command.go.patch
//! └── rustdeps/
//!     └── vendor-serde-lib.rs.patch
//! ```

use log::{debug, info, warn};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::edit_overlay::EditOverlay;

/// Information about a generated patch
#[derive(Debug, Clone)]
pub struct PatchInfo {
    /// The cell this patch belongs to
    pub cell_name: String,
    /// The relative path of the file within the cell
    pub relative_path: PathBuf,
    /// The patch file path
    pub patch_path: PathBuf,
    /// Size of the patch file in bytes
    pub size: u64,
}

/// Generates unified diff patches from edited files
pub struct PatchGenerator {
    /// Directory where patches are stored (e.g., `/repo/.turnkey/patches`)
    patches_dir: PathBuf,
    /// Mapping of cell name to its source path (Nix store path)
    cell_sources: HashMap<String, PathBuf>,
}

impl PatchGenerator {
    /// Create a new patch generator
    ///
    /// # Arguments
    ///
    /// * `patches_dir` - Directory where patches will be stored
    /// * `cell_sources` - Mapping of cell name to source path (original Nix store path)
    pub fn new(
        patches_dir: PathBuf,
        cell_sources: impl IntoIterator<Item = (String, PathBuf)>,
    ) -> Self {
        Self {
            patches_dir,
            cell_sources: cell_sources.into_iter().collect(),
        }
    }

    /// Get the patch file path for a given cell and relative path
    pub fn patch_path(&self, cell_name: &str, relative_path: &Path) -> PathBuf {
        let patch_name = Self::path_to_patch_name(relative_path);
        self.patches_dir.join(cell_name).join(patch_name)
    }

    /// Convert a relative path to a patch filename
    ///
    /// Replaces `/` with `-` and adds `.patch` extension.
    fn path_to_patch_name(path: &Path) -> String {
        let path_str = path.to_string_lossy();
        format!("{}.patch", path_str.replace('/', "-"))
    }

    /// Generate a patch for a single edited file
    ///
    /// Returns the patch info if successful, or None if the files are identical.
    pub fn generate_patch(
        &self,
        cell_name: &str,
        relative_path: &Path,
        edited_path: &Path,
    ) -> io::Result<Option<PatchInfo>> {
        // Get the original file path
        let source_path = self
            .cell_sources
            .get(cell_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cell source not found"))?;
        let original_path = source_path.join(relative_path);

        // Read both files
        let original_lines = read_lines(&original_path)?;
        let edited_lines = read_lines(edited_path)?;

        // Generate the diff
        let diff = unified_diff(
            &original_lines,
            &edited_lines,
            &format!("a/{}", relative_path.display()),
            &format!("b/{}", relative_path.display()),
        );

        // If no differences, return None
        if diff.is_empty() {
            return Ok(None);
        }

        // Write the patch file
        let patch_path = self.patch_path(cell_name, relative_path);
        if let Some(parent) = patch_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = File::create(&patch_path)?;
        file.write_all(diff.as_bytes())?;

        let size = diff.len() as u64;
        debug!(
            "Generated patch: {} ({} bytes)",
            patch_path.display(),
            size
        );

        Ok(Some(PatchInfo {
            cell_name: cell_name.to_string(),
            relative_path: relative_path.to_path_buf(),
            patch_path,
            size,
        }))
    }

    /// Generate patches for all edited files in an overlay
    ///
    /// Returns the list of generated patches.
    pub fn generate_all(&self, overlay: &EditOverlay) -> io::Result<Vec<PatchInfo>> {
        let edited_files = overlay.list_edited();
        let mut patches = Vec::new();

        for info in edited_files {
            let edited_path = overlay.overlay_path(&info.cell_name, &info.relative_path);

            match self.generate_patch(&info.cell_name, &info.relative_path, &edited_path) {
                Ok(Some(patch_info)) => {
                    info!(
                        "Generated patch for {}/{}",
                        info.cell_name,
                        info.relative_path.display()
                    );
                    patches.push(patch_info);
                }
                Ok(None) => {
                    debug!(
                        "No changes for {}/{}",
                        info.cell_name,
                        info.relative_path.display()
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to generate patch for {}/{}: {}",
                        info.cell_name,
                        info.relative_path.display(),
                        e
                    );
                }
            }
        }

        Ok(patches)
    }

    /// List all existing patches
    pub fn list_patches(&self) -> io::Result<Vec<PatchInfo>> {
        let mut patches = Vec::new();

        if !self.patches_dir.exists() {
            return Ok(patches);
        }

        // Iterate over cell directories
        for entry in fs::read_dir(&self.patches_dir)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }

            let cell_name = entry.file_name().to_string_lossy().to_string();

            // Iterate over patch files in this cell
            for patch_entry in fs::read_dir(entry.path())? {
                let patch_entry = patch_entry?;
                let patch_path = patch_entry.path();

                if patch_path.extension().map_or(false, |e| e == "patch") {
                    if let Some(relative_path) = Self::patch_name_to_path(&patch_path) {
                        let meta = fs::metadata(&patch_path)?;
                        patches.push(PatchInfo {
                            cell_name: cell_name.clone(),
                            relative_path,
                            patch_path,
                            size: meta.len(),
                        });
                    }
                }
            }
        }

        Ok(patches)
    }

    /// Convert a patch filename back to the original relative path
    fn patch_name_to_path(patch_path: &Path) -> Option<PathBuf> {
        let filename = patch_path.file_stem()?.to_str()?;
        // The filename has .patch extension removed, and - replaced with /
        // But we need to be careful: the original path might have had - in it
        // For now, assume - was /
        Some(PathBuf::from(filename.replace('-', "/")))
    }

    /// Delete a patch file
    pub fn delete_patch(&self, cell_name: &str, relative_path: &Path) -> io::Result<bool> {
        let patch_path = self.patch_path(cell_name, relative_path);
        if patch_path.exists() {
            fs::remove_file(&patch_path)?;
            debug!("Deleted patch: {}", patch_path.display());

            // Try to clean up empty directories
            if let Some(parent) = patch_path.parent() {
                let _ = fs::remove_dir(parent); // Ignore errors if not empty
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete all patches for a cell
    pub fn delete_cell_patches(&self, cell_name: &str) -> io::Result<usize> {
        let cell_dir = self.patches_dir.join(cell_name);
        if !cell_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in fs::read_dir(&cell_dir)? {
            let entry = entry?;
            if entry.path().extension().map_or(false, |e| e == "patch") {
                fs::remove_file(entry.path())?;
                count += 1;
            }
        }

        // Remove the cell directory if empty
        let _ = fs::remove_dir(&cell_dir);

        debug!("Deleted {} patches for cell '{}'", count, cell_name);
        Ok(count)
    }

    /// Get the patches directory
    pub fn patches_dir(&self) -> &Path {
        &self.patches_dir
    }
}

/// Read lines from a file
fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    reader.lines().collect()
}

/// Generate a unified diff between two sets of lines
///
/// This implements a simple line-by-line diff algorithm.
fn unified_diff(
    original: &[String],
    modified: &[String],
    original_label: &str,
    modified_label: &str,
) -> String {
    // Find the longest common subsequence using dynamic programming
    let lcs = longest_common_subsequence(original, modified);

    // Build the diff hunks
    let hunks = build_hunks(original, modified, &lcs);

    if hunks.is_empty() {
        return String::new();
    }

    // Format as unified diff
    let mut output = String::new();
    output.push_str(&format!("--- {}\n", original_label));
    output.push_str(&format!("+++ {}\n", modified_label));

    for hunk in hunks {
        output.push_str(&hunk.to_string());
    }

    output
}

/// A hunk in a unified diff
struct DiffHunk {
    /// Starting line in original (1-based)
    orig_start: usize,
    /// Number of lines from original
    orig_count: usize,
    /// Starting line in modified (1-based)
    mod_start: usize,
    /// Number of lines from modified
    mod_count: usize,
    /// The diff lines (context, additions, deletions)
    lines: Vec<DiffLine>,
}

impl std::fmt::Display for DiffHunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "@@ -{},{} +{},{} @@",
            self.orig_start, self.orig_count, self.mod_start, self.mod_count
        )?;
        for line in &self.lines {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

/// A single line in a diff
enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

impl std::fmt::Display for DiffLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffLine::Context(s) => write!(f, " {}", s),
            DiffLine::Added(s) => write!(f, "+{}", s),
            DiffLine::Removed(s) => write!(f, "-{}", s),
        }
    }
}

/// Find the longest common subsequence of two sequences
fn longest_common_subsequence(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let m = a.len();
    let n = b.len();

    if m == 0 || n == 0 {
        return Vec::new();
    }

    // Build LCS table
    let mut dp = vec![vec![0usize; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to find LCS
    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;

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

/// Build diff hunks from original, modified, and LCS
fn build_hunks(original: &[String], modified: &[String], lcs: &[(usize, usize)]) -> Vec<DiffHunk> {
    const CONTEXT_LINES: usize = 3;

    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;

    let mut orig_idx = 0;
    let mut mod_idx = 0;
    let mut lcs_idx = 0;

    // Track which lines are in LCS for quick lookup
    let lcs_orig: std::collections::HashSet<usize> = lcs.iter().map(|(i, _)| *i).collect();
    let lcs_mod: std::collections::HashSet<usize> = lcs.iter().map(|(_, j)| *j).collect();

    while orig_idx < original.len() || mod_idx < modified.len() {
        // Check if current positions are in LCS (context)
        let in_lcs = lcs_idx < lcs.len()
            && orig_idx == lcs[lcs_idx].0
            && mod_idx == lcs[lcs_idx].1;

        if in_lcs {
            // This is a common line (context)
            if let Some(ref mut hunk) = current_hunk {
                hunk.lines.push(DiffLine::Context(original[orig_idx].clone()));
                hunk.orig_count += 1;
                hunk.mod_count += 1;
            }
            orig_idx += 1;
            mod_idx += 1;
            lcs_idx += 1;
        } else {
            // We have a difference - start or extend a hunk
            if current_hunk.is_none() {
                // Start new hunk with context
                let context_start = orig_idx.saturating_sub(CONTEXT_LINES);
                let mod_context_start = mod_idx.saturating_sub(CONTEXT_LINES);

                let mut hunk = DiffHunk {
                    orig_start: context_start + 1, // 1-based
                    orig_count: 0,
                    mod_start: mod_context_start + 1,
                    mod_count: 0,
                    lines: Vec::new(),
                };

                // Add leading context
                for i in context_start..orig_idx {
                    if i < original.len() {
                        hunk.lines.push(DiffLine::Context(original[i].clone()));
                        hunk.orig_count += 1;
                        hunk.mod_count += 1;
                    }
                }

                current_hunk = Some(hunk);
            }

            let hunk = current_hunk.as_mut().unwrap();

            // Add removed lines (in original but not in LCS at current position)
            while orig_idx < original.len() && !lcs_orig.contains(&orig_idx) {
                hunk.lines.push(DiffLine::Removed(original[orig_idx].clone()));
                hunk.orig_count += 1;
                orig_idx += 1;
            }

            // Add added lines (in modified but not in LCS at current position)
            while mod_idx < modified.len() && !lcs_mod.contains(&mod_idx) {
                hunk.lines.push(DiffLine::Added(modified[mod_idx].clone()));
                hunk.mod_count += 1;
                mod_idx += 1;
            }
        }

        // Check if we should finalize the hunk (gap to next difference > 2*context)
        if let Some(ref hunk) = current_hunk {
            let next_diff_distance = if lcs_idx < lcs.len() {
                // Distance to next LCS entry (which would be context)
                // We want distance to next *difference* after the LCS
                if lcs_idx + 1 < lcs.len() {
                    (lcs[lcs_idx + 1].0 - orig_idx).min(lcs[lcs_idx + 1].1 - mod_idx)
                } else {
                    // No more LCS entries, check for trailing differences
                    (original.len() - orig_idx).max(modified.len() - mod_idx)
                }
            } else {
                0
            };

            // If we've processed all differences in this region, add trailing context
            // and finalize the hunk
            if lcs_idx >= lcs.len()
                || (orig_idx < original.len()
                    && mod_idx < modified.len()
                    && next_diff_distance > 2 * CONTEXT_LINES)
            {
                // We need to finalize - but only if we actually have differences
                let has_changes = hunk
                    .lines
                    .iter()
                    .any(|l| matches!(l, DiffLine::Added(_) | DiffLine::Removed(_)));

                if has_changes {
                    // Add trailing context
                    let mut hunk = current_hunk.take().unwrap();
                    let trailing_end = (orig_idx + CONTEXT_LINES).min(original.len());
                    while orig_idx < trailing_end && lcs_idx < lcs.len() && orig_idx == lcs[lcs_idx].0
                    {
                        hunk.lines.push(DiffLine::Context(original[orig_idx].clone()));
                        hunk.orig_count += 1;
                        hunk.mod_count += 1;
                        orig_idx += 1;
                        mod_idx += 1;
                        lcs_idx += 1;
                    }
                    hunks.push(hunk);
                } else {
                    current_hunk = None;
                }
            }
        }
    }

    // Finalize any remaining hunk
    if let Some(hunk) = current_hunk {
        let has_changes = hunk
            .lines
            .iter()
            .any(|l| matches!(l, DiffLine::Added(_) | DiffLine::Removed(_)));
        if has_changes {
            hunks.push(hunk);
        }
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_to_patch_name() {
        assert_eq!(
            PatchGenerator::path_to_patch_name(Path::new("vendor/github.com/foo/bar/lib.go")),
            "vendor-github.com-foo-bar-lib.go.patch"
        );
        assert_eq!(
            PatchGenerator::path_to_patch_name(Path::new("src/lib.rs")),
            "src-lib.rs.patch"
        );
    }

    #[test]
    fn test_unified_diff_no_changes() {
        let lines = vec!["line 1".into(), "line 2".into(), "line 3".into()];
        let diff = unified_diff(&lines, &lines, "a/file", "b/file");
        assert!(diff.is_empty());
    }

    #[test]
    fn test_unified_diff_single_change() {
        let original = vec!["line 1".into(), "line 2".into(), "line 3".into()];
        let modified = vec!["line 1".into(), "modified line 2".into(), "line 3".into()];

        let diff = unified_diff(&original, &modified, "a/file.txt", "b/file.txt");

        assert!(diff.contains("--- a/file.txt"));
        assert!(diff.contains("+++ b/file.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+modified line 2"));
    }

    #[test]
    fn test_unified_diff_additions() {
        let original = vec!["line 1".into(), "line 3".into()];
        let modified = vec!["line 1".into(), "line 2".into(), "line 3".into()];

        let diff = unified_diff(&original, &modified, "a/file", "b/file");

        assert!(diff.contains("+line 2"));
    }

    #[test]
    fn test_unified_diff_deletions() {
        let original = vec!["line 1".into(), "line 2".into(), "line 3".into()];
        let modified = vec!["line 1".into(), "line 3".into()];

        let diff = unified_diff(&original, &modified, "a/file", "b/file");

        assert!(diff.contains("-line 2"));
    }

    fn setup_test_env() -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let patches_dir = repo_root.join(".turnkey/patches");
        let cell_source = temp.path().join("nix/store/abc-godeps");

        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(cell_source.join("vendor/github.com/foo/bar")).unwrap();

        // Create original file
        let mut f = File::create(cell_source.join("vendor/github.com/foo/bar/lib.go")).unwrap();
        f.write_all(b"package bar\n\nfunc Hello() {\n    println(\"hello\")\n}\n")
            .unwrap();

        (temp, repo_root, patches_dir, cell_source)
    }

    #[test]
    fn test_generate_patch() {
        let (_temp, repo_root, patches_dir, cell_source) = setup_test_env();

        // Create an edited version
        let edits_dir = repo_root.join(".turnkey/edits");
        fs::create_dir_all(edits_dir.join("godeps/vendor/github.com/foo/bar")).unwrap();
        let mut f =
            File::create(edits_dir.join("godeps/vendor/github.com/foo/bar/lib.go")).unwrap();
        f.write_all(b"package bar\n\nfunc Hello() {\n    println(\"hello world\")\n}\n")
            .unwrap();

        let generator = PatchGenerator::new(
            patches_dir.clone(),
            vec![("godeps".into(), cell_source.clone())],
        );

        let patch_info = generator
            .generate_patch(
                "godeps",
                Path::new("vendor/github.com/foo/bar/lib.go"),
                &edits_dir.join("godeps/vendor/github.com/foo/bar/lib.go"),
            )
            .unwrap();

        assert!(patch_info.is_some());
        let info = patch_info.unwrap();
        assert_eq!(info.cell_name, "godeps");
        assert!(info.patch_path.exists());

        // Verify patch content
        let patch_content = fs::read_to_string(&info.patch_path).unwrap();
        assert!(patch_content.contains("-    println(\"hello\")"));
        assert!(patch_content.contains("+    println(\"hello world\")"));
    }

    #[test]
    fn test_list_patches() {
        let (_temp, _repo_root, patches_dir, cell_source) = setup_test_env();

        // Create some patch files
        fs::create_dir_all(patches_dir.join("godeps")).unwrap();
        fs::write(
            patches_dir.join("godeps/vendor-foo-lib.go.patch"),
            "--- a/file\n+++ b/file\n",
        )
        .unwrap();
        fs::write(
            patches_dir.join("godeps/vendor-bar-lib.go.patch"),
            "--- a/file\n+++ b/file\n",
        )
        .unwrap();

        let generator = PatchGenerator::new(patches_dir, vec![("godeps".into(), cell_source)]);

        let patches = generator.list_patches().unwrap();
        assert_eq!(patches.len(), 2);
    }

    #[test]
    fn test_delete_patch() {
        let (_temp, _repo_root, patches_dir, cell_source) = setup_test_env();

        // Create a patch file
        fs::create_dir_all(patches_dir.join("godeps")).unwrap();
        let patch_path = patches_dir.join("godeps/vendor-foo-lib.go.patch");
        fs::write(&patch_path, "--- a/file\n+++ b/file\n").unwrap();

        let generator = PatchGenerator::new(patches_dir, vec![("godeps".into(), cell_source)]);

        // Delete it
        let deleted = generator
            .delete_patch("godeps", Path::new("vendor/foo/lib.go"))
            .unwrap();
        assert!(deleted);
        assert!(!patch_path.exists());
    }
}
