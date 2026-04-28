//! CLI-based Nix client implementation
//!
//! Wraps the `nix` command-line binary. This is the implementation detail
//! that gets replaced when a proper Nix client library is available.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use log::{debug, info};

use crate::{NixClient, NixError};

/// Nix client that shells out to the `nix` CLI binary.
pub struct CliNixClient {
    /// Path to the flake (working directory for nix commands)
    flake_dir: PathBuf,
}

impl CliNixClient {
    /// Create a new CLI-based Nix client for the given flake directory.
    pub fn new(flake_dir: impl Into<PathBuf>) -> Self {
        Self {
            flake_dir: flake_dir.into(),
        }
    }

    /// Run a nix command and return its stdout.
    fn run_nix(&self, args: &[&str]) -> Result<String, NixError> {
        debug!("Running: nix {}", args.join(" "));

        let output = Command::new("nix")
            .args(args)
            .current_dir(&self.flake_dir)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    NixError::NotFound
                } else {
                    NixError::Exec {
                        message: e.to_string(),
                    }
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NixError::Command {
                message: stderr.trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl NixClient for CliNixClient {
    fn list_packages(&self, system: &str) -> Result<Vec<String>, NixError> {
        let attr = format!(".#packages.{}", system);
        let stdout = self.run_nix(&[
            "eval", &attr,
            "--apply", "builtins.attrNames",
            "--json",
            "--impure",
        ])?;

        serde_json::from_str(&stdout).map_err(|e| NixError::Parse {
            message: format!("failed to parse package list: {}", e),
        })
    }

    fn build(&self, packages: &[&str]) -> Result<HashMap<String, PathBuf>, NixError> {
        if packages.is_empty() {
            return Ok(HashMap::new());
        }

        let mut args = vec!["build", "--impure", "--no-link", "--print-out-paths"];
        let installables: Vec<String> = packages.iter().map(|p| format!(".#{}", p)).collect();
        for inst in &installables {
            args.push(inst);
        }

        info!("Building {} packages via nix", packages.len());
        let stdout = self.run_nix(&args)?;

        let paths: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

        if paths.len() != packages.len() {
            return Err(NixError::Parse {
                message: format!(
                    "expected {} output paths, got {}",
                    packages.len(),
                    paths.len()
                ),
            });
        }

        let mut result = HashMap::new();
        for (pkg, path) in packages.iter().zip(paths.iter()) {
            debug!("  {} -> {}", pkg, path);
            result.insert(pkg.to_string(), PathBuf::from(path));
        }

        Ok(result)
    }

    fn eval_json(&self, expr: &str) -> Result<serde_json::Value, NixError> {
        let stdout = self.run_nix(&[
            "eval", "--impure", "--json", "--expr", expr,
        ])?;

        serde_json::from_str(&stdout).map_err(|e| NixError::Parse {
            message: format!("failed to parse eval result: {}", e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_client_creation() {
        let client = CliNixClient::new("/tmp/test");
        assert_eq!(client.flake_dir, PathBuf::from("/tmp/test"));
    }
}
