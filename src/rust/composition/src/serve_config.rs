//! Service configuration for multi-mount daemon
//!
//! Reads `~/.config/turnkey/composed.toml` to configure which repositories
//! to mount and where. The service file is generic; this config is
//! user/device-specific.
//!
//! # Example
//!
//! ```toml
//! [[mounts]]
//! repo = "/Users/yann/src/github.com/firefly-engineering/turnkey"
//! mount_point = "/firefly/turnkey"
//!
//! [[mounts]]
//! repo = "/Users/yann/src/github.com/firefly-engineering/other-project"
//! mount_point = "/firefly/other"
//! backend = "fuse"
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Top-level service configuration
#[derive(Debug, Deserialize)]
pub struct ServeConfig {
    /// List of repositories to mount
    #[serde(rename = "mounts")]
    pub mounts: Vec<MountEntry>,
}

/// A single mount entry
#[derive(Debug, Deserialize)]
pub struct MountEntry {
    /// Path to the repository root (must contain a flake.nix)
    pub repo: PathBuf,

    /// Where to mount the composed view
    pub mount_point: PathBuf,

    /// Backend type: "auto", "fuse", or "symlink" (default: "auto")
    #[serde(default = "default_backend")]
    pub backend: String,

    /// Files/directories to exclude from the source pass-through
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_backend() -> String {
    "auto".to_string()
}

impl ServeConfig {
    /// Read config from a file
    pub fn read(path: &Path) -> Result<Self, ServeConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ServeConfigError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        toml::from_str(&content).map_err(|e| ServeConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })
    }

    /// Default config file path: ~/.config/turnkey/composed.toml
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("turnkey")
            .join("composed.toml")
    }
}

#[derive(Debug)]
pub enum ServeConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ServeConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeConfigError::Io { path, source } => {
                write!(f, "failed to read {}: {}", path.display(), source)
            }
            ServeConfigError::Parse { path, source } => {
                write!(f, "failed to parse {}: {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for ServeConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_mount() {
        let toml = r#"
[[mounts]]
repo = "/home/user/src/myproject"
mount_point = "/firefly/myproject"
"#;
        let config: ServeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.mounts.len(), 1);
        assert_eq!(config.mounts[0].repo, PathBuf::from("/home/user/src/myproject"));
        assert_eq!(config.mounts[0].backend, "auto");
    }

    #[test]
    fn test_parse_multiple_mounts() {
        let toml = r#"
[[mounts]]
repo = "/home/user/src/project-a"
mount_point = "/firefly/a"
backend = "fuse"

[[mounts]]
repo = "/home/user/src/project-b"
mount_point = "/firefly/b"
"#;
        let config: ServeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.mounts.len(), 2);
        assert_eq!(config.mounts[0].backend, "fuse");
        assert_eq!(config.mounts[1].backend, "auto");
    }
}
