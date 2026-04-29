//! Platform detection and FUSE availability checking
//!
//! This module provides platform-specific functionality for FUSE operations:
//! - Linux: Uses native FUSE via /dev/fuse
//! - macOS: Uses macFUSE (FSKit on macOS 26+, kext on older releases)
//!
//! # macFUSE on macOS
//!
//! macFUSE 5.2+ ships two backends: an FSKit-based file-system extension
//! (preferred on macOS 26+, no kernel extension required) and a legacy kext
//! (still bundled for older macOS releases). Both expose the upstream libfuse3
//! ABI via `/usr/local/lib/libfuse3.4.dylib`, so the same FFI bindings work
//! against either.
//!
//! Activation is *not* automatic and the FSKit path requires user approval in
//! System Settings; see `detect_macfuse_backend` for runtime detection and
//! [`MacFuseBackend::NotActivated`] for the activation guidance we surface.
//!
//! Installation: `brew install --cask macfuse`

use std::path::Path;
use std::process::Command;

/// Supported platforms for FUSE operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux with native FUSE support
    Linux,
    /// macOS with FUSE-T
    MacOS,
    /// Unsupported platform
    Unsupported,
}

impl Platform {
    /// Detect the current platform
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            Platform::Linux
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Platform::Unsupported
        }
    }

    /// Get a human-readable name for this platform
    pub fn name(&self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::MacOS => "macOS",
            Platform::Unsupported => "unsupported",
        }
    }
}

/// FUSE availability status
#[derive(Debug, Clone)]
pub enum FuseAvailability {
    /// FUSE is available and ready to use
    Available {
        /// Name of the FUSE implementation
        implementation: String,
        /// Version if known
        version: Option<String>,
    },
    /// FUSE is not installed
    NotInstalled {
        /// Instructions for installing FUSE
        install_instructions: String,
    },
    /// Platform doesn't support FUSE
    UnsupportedPlatform,
}

impl FuseAvailability {
    /// Check if FUSE is available
    pub fn is_available(&self) -> bool {
        matches!(self, FuseAvailability::Available { .. })
    }
}

/// Check FUSE availability on the current platform
pub fn check_fuse_availability() -> FuseAvailability {
    match Platform::detect() {
        Platform::Linux => check_fuse_linux(),
        Platform::MacOS => check_fuse_macos(),
        Platform::Unsupported => FuseAvailability::UnsupportedPlatform,
    }
}

/// Check FUSE availability on Linux
fn check_fuse_linux() -> FuseAvailability {
    // Check if /dev/fuse exists
    if Path::new("/dev/fuse").exists() {
        // Try to get version from fusermount
        let version = get_fusermount_version();
        FuseAvailability::Available {
            implementation: "FUSE".to_string(),
            version,
        }
    } else {
        FuseAvailability::NotInstalled {
            install_instructions: concat!(
                "Install FUSE on your Linux distribution:\n",
                "  Ubuntu/Debian: sudo apt install fuse3\n",
                "  Fedora: sudo dnf install fuse3\n",
                "  Arch: sudo pacman -S fuse3"
            )
            .to_string(),
        }
    }
}

/// Check FUSE availability on macOS (macFUSE preferred, FUSE-T accepted)
fn check_fuse_macos() -> FuseAvailability {
    // macFUSE installs the libfuse3 ABI dylib at this fixed path on both
    // Intel and Apple Silicon. FUSE-T also provides a libfuse3 ABI dylib at
    // the same path via its own symlink, so our FFI bindings work against
    // either backend. Reporting which one is *active* (FSKit vs kext) is the
    // job of `detect_macfuse_backend`.
    let libfuse3 = Path::new("/usr/local/lib/libfuse3.4.dylib");
    let macfuse_bundle = Path::new("/Library/Filesystems/macfuse.fs");
    let fuse_t_bundle = Path::new("/Library/Filesystems/fuse-t.fs");
    let fuse_t_lib = Path::new("/usr/local/lib/libfuse-t.dylib");

    if libfuse3.exists() && macfuse_bundle.exists() {
        FuseAvailability::Available {
            implementation: "macFUSE".to_string(),
            version: get_macfuse_version(),
        }
    } else if fuse_t_bundle.exists() || fuse_t_lib.exists() {
        FuseAvailability::Available {
            implementation: "FUSE-T".to_string(),
            version: get_fuse_t_version(),
        }
    } else {
        FuseAvailability::NotInstalled {
            install_instructions: concat!(
                "Install macFUSE on macOS (recommended; uses FSKit on macOS 26+):\n",
                "  brew install --cask macfuse\n",
                "\n",
                "Alternatively, install FUSE-T (NFS-based, deprecated by this project):\n",
                "  brew install macos-fuse-t/homebrew-cask/fuse-t"
            )
            .to_string(),
        }
    }
}

/// macFUSE-specific path constants. Centralised so callers stay in sync.
#[cfg(target_os = "macos")]
mod macfuse_paths {
    pub const BUNDLE: &str = "/Library/Filesystems/macfuse.fs";
    pub const INFO_PLIST: &str = "/Library/Filesystems/macfuse.fs/Contents/Info.plist";
    /// CFBundleIdentifier of the FSKit file-system extension (XPC `appex`).
    pub const FSKIT_BUNDLE_ID: &str = "io.macfuse.app.fsmodule.macfuse";
    /// CFBundleIdentifier prefix of the legacy kext (per-OS suffix appended,
    /// e.g. `.25` for the macOS 26 build).
    pub const KEXT_BUNDLE_ID_PREFIX: &str = "io.macfuse.filesystems.macfuse";
    /// `macfuse` GUI binary that exposes the `install` subcommand for
    /// registering the FSKit extension.
    pub const MACFUSE_APP_BIN: &str = "/Library/Filesystems/macfuse.fs/Contents/Resources/macfuse.app/Contents/MacOS/macfuse";
    /// Helper used for legacy kext loads (requires `sudo`).
    pub const LOAD_MACFUSE: &str = "/Library/Filesystems/macfuse.fs/Contents/Resources/load_macfuse";
}

/// FUSE-T paths. FUSE-T 1.2+ ships an FSKit `fsmodule` extension alongside
/// the older NFS-based `libfuse-t.dylib`. Either path is reachable via the
/// libfuse3 ABI exposed at `<fuse-t lib dir>/libfuse3.4.dylib`.
#[cfg(target_os = "macos")]
mod fuse_t_paths {
    pub const LIBFUSE3: &str = "/Library/Application Support/fuse-t/lib/libfuse3.4.dylib";
    pub const FSKIT_BUNDLE_ID: &str = "org.fuset.fskit-srv.module";
}

/// Which macOS FUSE backend, if any, is currently usable.
///
/// Returned by [`detect_macfuse_backend`] and consumed by the FUSE backend
/// before calling `fuse_mount`. Pre-checking is required because libfuse3's
/// `fuse_mount` (via macFUSE's `MFMount` framework) blocks indefinitely
/// waiting for a GUI approval prompt when no backend is active — fatal for a
/// daemon process.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacFuseBackend {
    /// An FSKit file-system extension (`com.apple.fskit.fsmodule`) is
    /// registered with `pluginkit`. The variant identifies which vendor's
    /// extension was found — macFUSE proper or FUSE-T's FskitSrvModule.
    FSKit {
        vendor: FuseVendor,
        bundle_id: String,
        /// Version string from pluginkit, e.g. "1.6" or "0.1.3".
        version: Option<String>,
    },
    /// Legacy kernel extension is currently loaded (kmutil reports it).
    Kext { bundle_id: String },
    /// FUSE is installed but no backend is active. `fuse_mount` would either
    /// hang on a System Settings approval prompt (macFUSE) or fail on the
    /// kext-load fallback (macFUSE on Apple Silicon Tahoe).
    NotActivated {
        /// macFUSE FSKit `appex` bundle is on disk.
        fskit_bundle_present: bool,
        /// At least one macFUSE kext bundle is on disk.
        kext_bundle_present: bool,
    },
    /// No FUSE library found (neither macFUSE nor FUSE-T).
    NotInstalled,
}

/// Identifies which userspace FUSE implementation provides a backend.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuseVendor {
    /// macFUSE 5.x — `io.macfuse.app.fsmodule.macfuse`.
    MacFuse,
    /// FUSE-T 1.2+ — `org.fuset.fskit-srv.module`. Currently the working
    /// FSKit path on Apple Silicon Tahoe.
    FuseT,
}

#[cfg(target_os = "macos")]
impl MacFuseBackend {
    /// Short label suitable for log lines.
    pub fn label(&self) -> &'static str {
        match self {
            MacFuseBackend::FSKit { vendor: FuseVendor::MacFuse, .. } => "macFUSE FSKit",
            MacFuseBackend::FSKit { vendor: FuseVendor::FuseT, .. } => "FUSE-T FSKit",
            MacFuseBackend::Kext { .. } => "macFUSE kext",
            MacFuseBackend::NotActivated { .. } => "macFUSE not activated",
            MacFuseBackend::NotInstalled => "macFUSE not installed",
        }
    }

    /// Multi-line activation instructions to surface to the user when the
    /// backend can't service a mount.
    pub fn activation_instructions(&self) -> String {
        match self {
            MacFuseBackend::FSKit { .. } | MacFuseBackend::Kext { .. } => String::new(),
            MacFuseBackend::NotInstalled => concat!(
                "macFUSE is not installed.\n",
                "Install it with: brew install --cask macfuse\n",
                "After install, open the macFUSE app once to register and enable\n",
                "its File System Extension in System Settings."
            )
            .to_string(),
            MacFuseBackend::NotActivated { fskit_bundle_present, kext_bundle_present } => {
                let mut msg = String::from(
                    "macFUSE is installed but no backend is active. Without activation,\n\
                     fuse_mount() will block waiting for a GUI approval prompt.\n\n",
                );
                if *fskit_bundle_present {
                    msg.push_str(
                        "Recommended (macOS 26+): activate the FSKit file-system extension.\n\
                         1) Run the macFUSE app and choose 'Register File System Extension':\n\
                            open /Library/Filesystems/macfuse.fs/Contents/Resources/macfuse.app\n\
                         2) In System Settings > General > Login Items & Extensions >\n\
                            File System Extensions, enable the macFUSE entry.\n",
                    );
                }
                if *kext_bundle_present {
                    if *fskit_bundle_present {
                        msg.push('\n');
                    }
                    msg.push_str(
                        "Alternative (legacy): load the macFUSE kernel extension.\n  \
                         sudo /Library/Filesystems/macfuse.fs/Contents/Resources/load_macfuse\n  \
                         (May require approving 'Benjamin Fleischer' in\n  \
                         System Settings > Privacy & Security.)\n",
                    );
                }
                msg
            }
        }
    }
}

/// Detect which macFUSE backend (FSKit or kext) is currently usable.
///
/// macFUSE's FSKit `fsmodule` extension uses the `EXExtensionPoint =
/// com.apple.fskit.fsmodule` extension point, which is enumerated by
/// `pluginkit`, *not* `systemextensionsctl` (that tool only covers
/// `OSSystemExtension` categories like network/cmio/driver). The legacy
/// kext is enumerated by `kmutil showloaded`.
///
/// Returns the first backend found, preferring FSKit, or
/// `NotActivated` / `NotInstalled` describing why a mount would fail.
/// Cheap (two short subprocess calls); intended to be called once before
/// `fuse_mount`.
///
/// Note: pluginkit reports *registered* extensions, not whether the user
/// has enabled them in System Settings — there's no CLI hook for the
/// election state. A registered-but-disabled extension may still cause
/// `fuse_mount` to surface an error from `MFMount.framework`, but it will
/// not block indefinitely on a GUI-registration prompt; that's the hang
/// this pre-flight check exists to prevent.
#[cfg(target_os = "macos")]
pub fn detect_macfuse_backend() -> MacFuseBackend {
    let macfuse_present = Path::new(macfuse_paths::BUNDLE).exists()
        || Path::new("/usr/local/lib/libfuse3.4.dylib").exists();
    let fuse_t_present = Path::new(fuse_t_paths::LIBFUSE3).exists();

    if !macfuse_present && !fuse_t_present {
        return MacFuseBackend::NotInstalled;
    }

    // Prefer macFUSE's FSKit module — that's what the binary links against
    // and what the `backend=fskit` mount option targets.
    if let Some(version) = pluginkit_fsmodule_registered(macfuse_paths::FSKIT_BUNDLE_ID) {
        return MacFuseBackend::FSKit {
            vendor: FuseVendor::MacFuse,
            bundle_id: macfuse_paths::FSKIT_BUNDLE_ID.to_string(),
            version: if version.is_empty() { None } else { Some(version) },
        };
    }

    if let Some(version) = pluginkit_fsmodule_registered(fuse_t_paths::FSKIT_BUNDLE_ID) {
        return MacFuseBackend::FSKit {
            vendor: FuseVendor::FuseT,
            bundle_id: fuse_t_paths::FSKIT_BUNDLE_ID.to_string(),
            version: if version.is_empty() { None } else { Some(version) },
        };
    }

    if let Some(bundle_id) = kmutil_loaded_macfuse_kext(macfuse_paths::KEXT_BUNDLE_ID_PREFIX) {
        return MacFuseBackend::Kext { bundle_id };
    }

    let fskit_bundle_present = Path::new(macfuse_paths::MACFUSE_APP_BIN).exists();
    let kext_bundle_present = Path::new(macfuse_paths::LOAD_MACFUSE).exists()
        && std::fs::read_dir("/Library/Filesystems/macfuse.fs/Contents/Extensions")
            .map(|it| it.flatten().any(|e| e.path().join("macfuse.kext").exists()))
            .unwrap_or(false);

    MacFuseBackend::NotActivated {
        fskit_bundle_present,
        kext_bundle_present,
    }
}

/// If `bundle_id` is a registered FSKit `fsmodule` extension, return its
/// version string from `pluginkit`. Returns `Some("")` when the extension
/// is registered but has no parsable version.
#[cfg(target_os = "macos")]
fn pluginkit_fsmodule_registered(bundle_id: &str) -> Option<String> {
    let output = Command::new("pluginkit")
        .args([
            "-m",
            "-p",
            "com.apple.fskit.fsmodule",
            "-i",
            bundle_id,
        ])
        .output()
        .ok()?;
    // pluginkit always returns 0; the only signal is non-empty stdout
    // listing the bundle.
    parse_pluginkit_match(&String::from_utf8_lossy(&output.stdout), bundle_id)
}

/// Pure parser for `pluginkit -m -p ... -i <bundle_id>` output.
///
/// Format per match (one or more lines, each with a leading election char
/// like `-`, `+`, or space):
///   `-    io.macfuse.app.fsmodule.macfuse(1.6)`
/// Returns the version inside the parentheses (or empty string if none),
/// or `None` if no line contains the bundle ID.
#[cfg(target_os = "macos")]
fn parse_pluginkit_match(stdout: &str, bundle_id: &str) -> Option<String> {
    for line in stdout.lines() {
        if !line.contains(bundle_id) {
            continue;
        }
        // After the bundle ID we expect "(version)" or "((null))".
        let after_bid = line.split(bundle_id).nth(1)?.trim_start();
        // Built-in plugins serialize as `bundleID((null))` — treat as empty.
        if after_bid.starts_with("((null))") {
            return Some(String::new());
        }
        let version = after_bid
            .strip_prefix('(')
            .and_then(|s| s.split(')').next())
            .map(|s| s.to_string())
            .unwrap_or_default();
        return Some(version);
    }
    None
}

/// If a kext whose bundle ID starts with `prefix` is loaded, return its full
/// bundle ID. Parses `kmutil showloaded --list-only`.
#[cfg(target_os = "macos")]
fn kmutil_loaded_macfuse_kext(prefix: &str) -> Option<String> {
    let output = Command::new("kmutil")
        .args(["showloaded", "--list-only"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_kmutil_loaded(&String::from_utf8_lossy(&output.stdout), prefix)
}

/// Pure parser for `kmutil showloaded --list-only` output. Returns the first
/// whitespace-delimited token starting with `prefix`.
#[cfg(target_os = "macos")]
fn parse_kmutil_loaded(stdout: &str, prefix: &str) -> Option<String> {
    for line in stdout.lines() {
        for token in line.split_whitespace() {
            if token.starts_with(prefix) {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Read macFUSE's bundle version from its Info.plist. Best-effort.
#[cfg(target_os = "macos")]
fn get_macfuse_version() -> Option<String> {
    let plist = macfuse_paths::INFO_PLIST;
    if !Path::new(plist).exists() {
        return None;
    }
    let output = Command::new("defaults")
        .args(["read", plist, "CFBundleShortVersionString"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(format!("macFUSE {}", v))
    }
}

/// Get fusermount version on Linux
fn get_fusermount_version() -> Option<String> {
    // Try fusermount3 first
    if let Ok(output) = Command::new("fusermount3").arg("-V").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Some(stdout.trim().to_string());
        }
    }

    // Fall back to fusermount
    if let Ok(output) = Command::new("fusermount").arg("-V").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Some(stdout.trim().to_string());
        }
    }

    None
}

/// Get FUSE-T version on macOS
fn get_fuse_t_version() -> Option<String> {
    // FUSE-T stores version in Info.plist
    let plist_path = "/Library/Filesystems/fuse-t.fs/Contents/Info.plist";
    if Path::new(plist_path).exists() {
        // Use defaults to read the version
        if let Ok(output) = Command::new("defaults")
            .args(["read", plist_path, "CFBundleShortVersionString"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                return Some(format!("FUSE-T {}", version.trim()));
            }
        }
    }
    None
}

/// Unmount a FUSE filesystem
///
/// Uses the appropriate command for the current platform:
/// - Linux: fusermount3 -u or fusermount -u
/// - macOS: umount
///
/// Returns an error message if unmount fails.
pub fn unmount(mount_point: &Path) -> Result<(), String> {
    match Platform::detect() {
        Platform::Linux => unmount_linux(mount_point),
        Platform::MacOS => unmount_macos(mount_point),
        Platform::Unsupported => Err("FUSE not supported on this platform".to_string()),
    }
}

/// Unmount on Linux using fusermount
fn unmount_linux(mount_point: &Path) -> Result<(), String> {
    // Try fusermount3 first
    let result = Command::new("fusermount3")
        .arg("-u")
        .arg(mount_point)
        .output();

    match result {
        Ok(output) if output.status.success() => return Ok(()),
        Ok(output) => {
            // fusermount3 failed, try fusermount as fallback
            let result2 = Command::new("fusermount")
                .arg("-u")
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                Ok(output2) => {
                    let stderr = String::from_utf8_lossy(&output2.stderr);
                    Err(format!("fusermount failed: {}", stderr.trim()))
                }
                Err(e) => {
                    // Both failed, report the original error
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!(
                        "fusermount3 failed: {}, fusermount error: {}",
                        stderr.trim(),
                        e
                    ))
                }
            }
        }
        Err(_) => {
            // fusermount3 not found, try fusermount
            let result2 = Command::new("fusermount")
                .arg("-u")
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                Ok(output2) => {
                    let stderr = String::from_utf8_lossy(&output2.stderr);
                    Err(format!("fusermount failed: {}", stderr.trim()))
                }
                Err(e) => Err(format!("fusermount not found: {}", e)),
            }
        }
    }
}

/// Unmount on macOS using umount
fn unmount_macos(mount_point: &Path) -> Result<(), String> {
    // On macOS, use the standard umount command
    let result = Command::new("umount").arg(mount_point).output();

    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If regular umount fails, try with diskutil (handles force unmount better)
            let result2 = Command::new("diskutil")
                .args(["unmount", "force"])
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                _ => Err(format!("umount failed: {}", stderr.trim())),
            }
        }
        Err(e) => Err(format!("umount command failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detect() {
        let platform = Platform::detect();

        #[cfg(target_os = "linux")]
        assert_eq!(platform, Platform::Linux);

        #[cfg(target_os = "macos")]
        assert_eq!(platform, Platform::MacOS);

        // Platform should have a name
        assert!(!platform.name().is_empty());
    }

    #[test]
    fn test_platform_name() {
        assert_eq!(Platform::Linux.name(), "Linux");
        assert_eq!(Platform::MacOS.name(), "macOS");
        assert_eq!(Platform::Unsupported.name(), "unsupported");
    }

    #[test]
    fn test_fuse_availability_is_available() {
        let available = FuseAvailability::Available {
            implementation: "FUSE".to_string(),
            version: Some("3.10.0".to_string()),
        };
        assert!(available.is_available());

        let not_installed = FuseAvailability::NotInstalled {
            install_instructions: "test".to_string(),
        };
        assert!(!not_installed.is_available());

        let unsupported = FuseAvailability::UnsupportedPlatform;
        assert!(!unsupported.is_available());
    }

    #[test]
    fn test_check_fuse_availability() {
        // Just test that it doesn't panic
        let availability = check_fuse_availability();

        // On CI/test systems, FUSE might not be available
        match availability {
            FuseAvailability::Available { implementation, .. } => {
                assert!(!implementation.is_empty());
            }
            FuseAvailability::NotInstalled {
                install_instructions,
            } => {
                assert!(!install_instructions.is_empty());
            }
            FuseAvailability::UnsupportedPlatform => {
                // Expected on unsupported platforms
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_pluginkit_match_with_version() {
        let sample = "     io.macfuse.app.fsmodule.macfuse(1.6)\n";
        assert_eq!(
            parse_pluginkit_match(sample, "io.macfuse.app.fsmodule.macfuse").as_deref(),
            Some("1.6")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_pluginkit_match_with_election_prefix() {
        // pluginkit -mv prefixes lines with the election state char.
        let sample = "-    io.macfuse.app.fsmodule.macfuse(1.6)\tUUID\ttimestamp\tpath\n (1 plug-in)\n";
        assert_eq!(
            parse_pluginkit_match(sample, "io.macfuse.app.fsmodule.macfuse").as_deref(),
            Some("1.6")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_pluginkit_match_null_version() {
        // Built-in plugins have "(null)" as their version; treat as empty.
        let sample = "     com.apple.fskit.exfat((null))\n";
        assert_eq!(
            parse_pluginkit_match(sample, "com.apple.fskit.exfat").as_deref(),
            Some("")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_pluginkit_match_absent() {
        // pluginkit produces empty stdout when the bundle ID isn't registered.
        assert_eq!(parse_pluginkit_match("", "io.macfuse.app.fsmodule.macfuse"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_pluginkit_match_unrelated_bundle() {
        let sample = "     com.apple.fskit.exfat((null))\n";
        assert_eq!(
            parse_pluginkit_match(sample, "io.macfuse.app.fsmodule.macfuse"),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_kmutil_loaded_finds_macfuse() {
        let sample = "\
No variant specified, falling back to release
   10   19 0xfffffe0007c32880 0x1e9b0 0x1e9b0 com.apple.kec.corecrypto (26.0) UUID <9 8 7>
  240    1 0xfffffe000a000000 0x12000 0x12000 io.macfuse.filesystems.macfuse.25 (5.1.3) UUID <>
";
        assert_eq!(
            parse_kmutil_loaded(sample, "io.macfuse.filesystems.macfuse"),
            Some("io.macfuse.filesystems.macfuse.25".to_string())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_kmutil_loaded_returns_none_when_absent() {
        let sample = "\
   10   19 0xfffffe0007c32880 0x1e9b0 0x1e9b0 com.apple.kec.corecrypto (26.0) UUID <>
";
        assert_eq!(
            parse_kmutil_loaded(sample, "io.macfuse.filesystems.macfuse"),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macfuse_backend_label_and_instructions() {
        let active_macfuse = MacFuseBackend::FSKit {
            vendor: FuseVendor::MacFuse,
            bundle_id: "x".to_string(),
            version: None,
        };
        assert_eq!(active_macfuse.label(), "macFUSE FSKit");
        assert!(active_macfuse.activation_instructions().is_empty());

        let active_fuse_t = MacFuseBackend::FSKit {
            vendor: FuseVendor::FuseT,
            bundle_id: "y".to_string(),
            version: Some("1.0".to_string()),
        };
        assert_eq!(active_fuse_t.label(), "FUSE-T FSKit");

        let na = MacFuseBackend::NotActivated {
            fskit_bundle_present: true,
            kext_bundle_present: true,
        };
        let msg = na.activation_instructions();
        assert!(msg.contains("File System Extension"));
        assert!(msg.contains("load_macfuse"));
    }
}
