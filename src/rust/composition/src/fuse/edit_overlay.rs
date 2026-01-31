//! Copy-on-write overlay for external dependency editing
//!
//! This module implements a copy-on-write overlay that allows editing files
//! in external dependency cells (which are backed by read-only Nix store paths).
//!
//! # How it works
//!
//! When a file in a dependency cell is written:
//!
//! 1. The file is checked against the overlay directory (`.turnkey/edits/`)
//! 2. If not present, the original file is copied from the Nix store
//! 3. The modification is applied to the overlay copy
//! 4. Subsequent reads return the overlay copy instead of the original
//!
//! # Directory Structure
//!
//! ```text
//! .turnkey/
//! ├── edits/                    # Modified files (copy-on-write)
//! │   ├── godeps/
//! │   │   └── vendor/
//! │   │       └── github.com/spf13/cobra/
//! │   │           └── command.go
//! │   └── rustdeps/
//! │       └── vendor/
//! │           └── serde/
//! │               └── lib.rs
//! └── patches/                  # Generated patches (separate concern)
//!     └── godeps/
//!         └── github.com-spf13-cobra.patch
//! ```

use log::debug;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

/// Metadata for an edited file
#[derive(Debug, Clone)]
pub struct EditedFileInfo {
    /// The cell this file belongs to
    pub cell_name: String,
    /// The relative path within the cell (e.g., "vendor/github.com/spf13/cobra/command.go")
    pub relative_path: PathBuf,
    /// Size of the edited file
    pub size: u64,
    /// Last modification time
    pub mtime: SystemTime,
}

/// Copy-on-write overlay for dependency editing
///
/// The overlay manages modified files in a separate directory, allowing
/// reads to transparently return either the edited version or the original
/// from the Nix store.
pub struct EditOverlay {
    /// Base directory for edits (e.g., `/repo/.turnkey/edits`)
    edits_dir: PathBuf,
    /// Set of cells that are editable
    editable_cells: HashSet<String>,
    /// Cache of known edited files: (cell_name, relative_path) -> EditedFileInfo
    edited_files: RwLock<HashMap<(String, PathBuf), EditedFileInfo>>,
}

impl EditOverlay {
    /// Create a new edit overlay
    ///
    /// # Arguments
    ///
    /// * `edits_dir` - Base directory for storing edited files (e.g., `.turnkey/edits`)
    /// * `editable_cells` - Names of cells that can be edited
    pub fn new(edits_dir: PathBuf, editable_cells: impl IntoIterator<Item = String>) -> Self {
        Self {
            edits_dir,
            editable_cells: editable_cells.into_iter().collect(),
            edited_files: RwLock::new(HashMap::new()),
        }
    }

    /// Check if a cell is editable
    pub fn is_cell_editable(&self, cell_name: &str) -> bool {
        self.editable_cells.contains(cell_name)
    }

    /// Get the overlay path for a file in a cell
    ///
    /// Returns the path where the edited version would be stored, regardless
    /// of whether the file has been edited.
    pub fn overlay_path(&self, cell_name: &str, relative_path: &Path) -> PathBuf {
        self.edits_dir.join(cell_name).join(relative_path)
    }

    /// Check if a file has been edited
    pub fn is_edited(&self, cell_name: &str, relative_path: &Path) -> bool {
        // First check cache
        {
            let cache = self.edited_files.read().unwrap();
            if cache.contains_key(&(cell_name.to_string(), relative_path.to_path_buf())) {
                return true;
            }
        }

        // Check filesystem
        let overlay_path = self.overlay_path(cell_name, relative_path);
        if overlay_path.exists() {
            // Update cache
            if let Ok(meta) = fs::metadata(&overlay_path) {
                let info = EditedFileInfo {
                    cell_name: cell_name.to_string(),
                    relative_path: relative_path.to_path_buf(),
                    size: meta.len(),
                    mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                };
                let mut cache = self.edited_files.write().unwrap();
                cache.insert((cell_name.to_string(), relative_path.to_path_buf()), info);
            }
            return true;
        }

        false
    }

    /// Get the path to read from (overlay if edited, original otherwise)
    ///
    /// Returns `Some(overlay_path)` if the file has been edited, `None` if
    /// the original should be used.
    pub fn get_read_path(&self, cell_name: &str, relative_path: &Path) -> Option<PathBuf> {
        if self.is_edited(cell_name, relative_path) {
            Some(self.overlay_path(cell_name, relative_path))
        } else {
            None
        }
    }

    /// Copy the original file to the overlay (for copy-on-write)
    ///
    /// This is called when a file is about to be modified for the first time.
    /// It copies the original content from the Nix store to the overlay directory.
    pub fn copy_to_overlay(
        &self,
        cell_name: &str,
        relative_path: &Path,
        original_path: &Path,
    ) -> io::Result<PathBuf> {
        if !self.is_cell_editable(cell_name) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Cell '{}' is not editable", cell_name),
            ));
        }

        let overlay_path = self.overlay_path(cell_name, relative_path);

        // Create parent directories
        if let Some(parent) = overlay_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Copy the original file
        debug!(
            "Copying '{}' to overlay at '{}'",
            original_path.display(),
            overlay_path.display()
        );
        fs::copy(original_path, &overlay_path)?;

        // Update cache
        if let Ok(meta) = fs::metadata(&overlay_path) {
            let info = EditedFileInfo {
                cell_name: cell_name.to_string(),
                relative_path: relative_path.to_path_buf(),
                size: meta.len(),
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            };
            let mut cache = self.edited_files.write().unwrap();
            cache.insert((cell_name.to_string(), relative_path.to_path_buf()), info);
        }

        Ok(overlay_path)
    }

    /// Ensure a file exists in the overlay for writing
    ///
    /// If the file hasn't been edited yet, copies it from the original.
    /// Returns the overlay path to write to.
    pub fn ensure_overlay(
        &self,
        cell_name: &str,
        relative_path: &Path,
        original_path: &Path,
    ) -> io::Result<PathBuf> {
        if self.is_edited(cell_name, relative_path) {
            Ok(self.overlay_path(cell_name, relative_path))
        } else {
            self.copy_to_overlay(cell_name, relative_path, original_path)
        }
    }

    /// Write data to an overlay file
    ///
    /// Handles copy-on-write: if the file hasn't been edited before, copies
    /// the original first, then applies the write.
    pub fn write(
        &self,
        cell_name: &str,
        relative_path: &Path,
        original_path: &Path,
        offset: i64,
        data: &[u8],
    ) -> io::Result<u32> {
        let overlay_path = self.ensure_overlay(cell_name, relative_path, original_path)?;

        // Open for writing
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&overlay_path)?;

        // Seek to offset and write
        file.seek(SeekFrom::Start(offset as u64))?;
        let written = file.write(data)?;

        // Update cache
        if let Ok(meta) = fs::metadata(&overlay_path) {
            let info = EditedFileInfo {
                cell_name: cell_name.to_string(),
                relative_path: relative_path.to_path_buf(),
                size: meta.len(),
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            };
            let mut cache = self.edited_files.write().unwrap();
            cache.insert((cell_name.to_string(), relative_path.to_path_buf()), info);
        }

        Ok(written as u32)
    }

    /// Read from an overlay file (or original if not edited)
    ///
    /// Returns the data read and whether it was from the overlay.
    pub fn read(
        &self,
        cell_name: &str,
        relative_path: &Path,
        original_path: &Path,
        offset: i64,
        size: u32,
    ) -> io::Result<(Vec<u8>, bool)> {
        let (path, is_overlay) = if let Some(overlay_path) =
            self.get_read_path(cell_name, relative_path)
        {
            (overlay_path, true)
        } else {
            (original_path.to_path_buf(), false)
        };

        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(offset as u64))?;

        let mut buf = vec![0u8; size as usize];
        let n = file.read(&mut buf)?;
        buf.truncate(n);

        Ok((buf, is_overlay))
    }

    /// Truncate an overlay file
    pub fn truncate(
        &self,
        cell_name: &str,
        relative_path: &Path,
        original_path: &Path,
        size: u64,
    ) -> io::Result<()> {
        let overlay_path = self.ensure_overlay(cell_name, relative_path, original_path)?;
        let file = OpenOptions::new().write(true).open(&overlay_path)?;
        file.set_len(size)?;

        // Update cache
        let info = EditedFileInfo {
            cell_name: cell_name.to_string(),
            relative_path: relative_path.to_path_buf(),
            size,
            mtime: SystemTime::now(),
        };
        let mut cache = self.edited_files.write().unwrap();
        cache.insert((cell_name.to_string(), relative_path.to_path_buf()), info);

        Ok(())
    }

    /// List all edited files
    pub fn list_edited(&self) -> Vec<EditedFileInfo> {
        // Scan the edits directory to ensure cache is up to date
        self.scan_edits_dir();

        let cache = self.edited_files.read().unwrap();
        cache.values().cloned().collect()
    }

    /// List edited files for a specific cell
    pub fn list_edited_in_cell(&self, cell_name: &str) -> Vec<EditedFileInfo> {
        self.list_edited()
            .into_iter()
            .filter(|info| info.cell_name == cell_name)
            .collect()
    }

    /// Scan the edits directory and update the cache
    fn scan_edits_dir(&self) {
        if !self.edits_dir.exists() {
            return;
        }

        // Walk through cell directories
        if let Ok(entries) = fs::read_dir(&self.edits_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(cell_name) = entry.file_name().to_str() {
                        self.scan_cell_dir(cell_name, &entry.path(), PathBuf::new());
                    }
                }
            }
        }
    }

    /// Recursively scan a cell's edit directory
    fn scan_cell_dir(&self, cell_name: &str, base_path: &Path, relative: PathBuf) {
        let current = base_path.join(&relative);
        if let Ok(entries) = fs::read_dir(&current) {
            for entry in entries.flatten() {
                let entry_relative = relative.join(entry.file_name());
                if entry.path().is_dir() {
                    self.scan_cell_dir(cell_name, base_path, entry_relative);
                } else if let Ok(meta) = entry.metadata() {
                    let info = EditedFileInfo {
                        cell_name: cell_name.to_string(),
                        relative_path: entry_relative.clone(),
                        size: meta.len(),
                        mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    };
                    let mut cache = self.edited_files.write().unwrap();
                    cache.insert((cell_name.to_string(), entry_relative), info);
                }
            }
        }
    }

    /// Revert an edited file (delete the overlay copy)
    pub fn revert(&self, cell_name: &str, relative_path: &Path) -> io::Result<bool> {
        let overlay_path = self.overlay_path(cell_name, relative_path);

        if overlay_path.exists() {
            fs::remove_file(&overlay_path)?;

            // Remove from cache
            let mut cache = self.edited_files.write().unwrap();
            cache.remove(&(cell_name.to_string(), relative_path.to_path_buf()));

            // Clean up empty parent directories
            self.cleanup_empty_dirs(&overlay_path);

            debug!(
                "Reverted edit: {}/{}",
                cell_name,
                relative_path.display()
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Revert all edits in a cell
    pub fn revert_cell(&self, cell_name: &str) -> io::Result<usize> {
        let cell_dir = self.edits_dir.join(cell_name);
        if !cell_dir.exists() {
            return Ok(0);
        }

        let edited = self.list_edited_in_cell(cell_name);
        let count = edited.len();

        // Remove the entire cell directory
        fs::remove_dir_all(&cell_dir)?;

        // Clear cache entries for this cell
        let mut cache = self.edited_files.write().unwrap();
        cache.retain(|(cn, _), _| cn != cell_name);

        debug!("Reverted {} edits in cell '{}'", count, cell_name);
        Ok(count)
    }

    /// Clean up empty parent directories after reverting
    fn cleanup_empty_dirs(&self, path: &Path) {
        let mut current = path.parent();
        while let Some(dir) = current {
            // Stop at the edits_dir boundary
            if dir == self.edits_dir {
                break;
            }

            // Try to remove if empty
            if fs::remove_dir(dir).is_err() {
                // Directory not empty or other error, stop
                break;
            }

            current = dir.parent();
        }
    }

    /// Get the edits directory path
    pub fn edits_dir(&self) -> &Path {
        &self.edits_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, PathBuf, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let edits_dir = temp_dir.path().join(".turnkey/edits");
        let nix_store = temp_dir.path().join("nix/store/abc-godeps");

        // Create a mock Nix store file
        fs::create_dir_all(nix_store.join("vendor/github.com/foo/bar")).unwrap();
        let mut f = File::create(nix_store.join("vendor/github.com/foo/bar/lib.go")).unwrap();
        f.write_all(b"package bar\n\nfunc Hello() {}\n").unwrap();

        (temp_dir, edits_dir, nix_store)
    }

    #[test]
    fn test_overlay_path() {
        let overlay = EditOverlay::new(PathBuf::from("/repo/.turnkey/edits"), vec!["godeps".into()]);
        let path = overlay.overlay_path("godeps", Path::new("vendor/github.com/foo/bar/lib.go"));
        assert_eq!(
            path,
            PathBuf::from("/repo/.turnkey/edits/godeps/vendor/github.com/foo/bar/lib.go")
        );
    }

    #[test]
    fn test_is_cell_editable() {
        let overlay = EditOverlay::new(
            PathBuf::from("/repo/.turnkey/edits"),
            vec!["godeps".into(), "rustdeps".into()],
        );
        assert!(overlay.is_cell_editable("godeps"));
        assert!(overlay.is_cell_editable("rustdeps"));
        assert!(!overlay.is_cell_editable("pydeps"));
    }

    #[test]
    fn test_copy_on_write() {
        let (_temp, edits_dir, nix_store) = setup_test_env();
        let overlay = EditOverlay::new(edits_dir.clone(), vec!["godeps".into()]);

        let relative = PathBuf::from("vendor/github.com/foo/bar/lib.go");
        let original = nix_store.join(&relative);

        // Initially not edited
        assert!(!overlay.is_edited("godeps", &relative));

        // Copy to overlay
        let overlay_path = overlay
            .copy_to_overlay("godeps", &relative, &original)
            .unwrap();
        assert!(overlay_path.exists());

        // Now it's edited
        assert!(overlay.is_edited("godeps", &relative));

        // Content should match
        let mut content = String::new();
        File::open(&overlay_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert!(content.contains("package bar"));
    }

    #[test]
    fn test_write_creates_overlay() {
        let (_temp, edits_dir, nix_store) = setup_test_env();
        let overlay = EditOverlay::new(edits_dir.clone(), vec!["godeps".into()]);

        let relative = PathBuf::from("vendor/github.com/foo/bar/lib.go");
        let original = nix_store.join(&relative);

        // Write to the file (triggers copy-on-write)
        let new_content = b"// Modified\npackage bar\n";
        overlay
            .write("godeps", &relative, &original, 0, new_content)
            .unwrap();

        // Should be edited now
        assert!(overlay.is_edited("godeps", &relative));

        // Read back should return new content
        let (data, is_overlay) = overlay
            .read("godeps", &relative, &original, 0, 1024)
            .unwrap();
        assert!(is_overlay);
        assert!(String::from_utf8_lossy(&data).contains("Modified"));
    }

    #[test]
    fn test_revert_edit() {
        let (_temp, edits_dir, nix_store) = setup_test_env();
        let overlay = EditOverlay::new(edits_dir.clone(), vec!["godeps".into()]);

        let relative = PathBuf::from("vendor/github.com/foo/bar/lib.go");
        let original = nix_store.join(&relative);

        // Create an edit
        overlay
            .copy_to_overlay("godeps", &relative, &original)
            .unwrap();
        assert!(overlay.is_edited("godeps", &relative));

        // Revert it
        let reverted = overlay.revert("godeps", &relative).unwrap();
        assert!(reverted);
        assert!(!overlay.is_edited("godeps", &relative));
    }

    #[test]
    fn test_non_editable_cell_rejected() {
        let (_temp, edits_dir, nix_store) = setup_test_env();
        let overlay = EditOverlay::new(edits_dir.clone(), vec!["godeps".into()]);

        let relative = PathBuf::from("vendor/serde/lib.rs");
        let original = nix_store.join(&relative);

        // Try to copy for a non-editable cell
        let result = overlay.copy_to_overlay("rustdeps", &relative, &original);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not editable"));
    }

    #[test]
    fn test_list_edited_files() {
        let (_temp, edits_dir, nix_store) = setup_test_env();
        let overlay = EditOverlay::new(edits_dir.clone(), vec!["godeps".into()]);

        // Create some edits
        let relative = PathBuf::from("vendor/github.com/foo/bar/lib.go");
        let original = nix_store.join(&relative);
        overlay
            .copy_to_overlay("godeps", &relative, &original)
            .unwrap();

        let edited = overlay.list_edited();
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0].cell_name, "godeps");
        assert_eq!(edited[0].relative_path, relative);
    }
}
