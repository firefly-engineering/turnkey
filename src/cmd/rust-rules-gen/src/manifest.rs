//! What a vendored crate's rules.star takes from its own Cargo.toml.
//!
//! Vendored crates.io manifests are normalized: nothing is inherited from a
//! workspace, so a crate's directory is all there is to read.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use toml::{Table, Value};

/// A crate's manifest, and the directory holding it
pub struct Manifest<'a> {
    table: Table,
    dir: &'a Path,
}

impl<'a> Manifest<'a> {
    /// The crate's Cargo.toml, or an empty manifest when it has none
    pub fn read(dir: &'a Path) -> Result<Manifest<'a>> {
        let path = dir.join("Cargo.toml");
        let table = if path.exists() {
            std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?
                .parse()
                .with_context(|| format!("Failed to parse {}", path.display()))?
        } else {
            Table::new()
        };
        Ok(Manifest { table, dir })
    }

    fn section(&self, name: &str) -> Option<&Table> {
        self.table.get(name).and_then(Value::as_table)
    }

    fn package(&self, key: &str) -> Option<&Value> {
        self.section("package").and_then(|p| p.get(key))
    }

    fn package_str(&self, key: &str) -> Option<&str> {
        self.package(key).and_then(Value::as_str)
    }

    /// The package name, or the directory's name up to its `@version`
    pub fn crate_name(&self) -> String {
        match self.package_str("name") {
            Some(name) => name.to_string(),
            None => {
                let dir = self.dir.file_name().unwrap_or_default().to_string_lossy();
                dir.split('@').next().unwrap_or_default().to_string()
            }
        }
    }

    /// The edition: 2015 when unset, 2021 when inherited from a workspace
    /// the crate doesn't come with
    pub fn edition(&self) -> String {
        match self.package("edition") {
            None => "2015".to_string(),
            Some(Value::String(edition)) => edition.clone(),
            Some(Value::Table(t)) if t.get("workspace").and_then(Value::as_bool) == Some(true) => {
                "2021".to_string()
            }
            Some(other) => other.to_string(),
        }
    }

    /// The library's root: [lib] path, or src/lib.rs or lib.rs when it
    /// exists. Always explicit, so Buck2 never infers one from srcs.
    pub fn lib_path(&self) -> Option<String> {
        if let Some(path) = self
            .section("lib")
            .and_then(|l| l.get("path"))
            .and_then(Value::as_str)
        {
            return Some(path.to_string());
        }
        ["src/lib.rs", "lib.rs"]
            .into_iter()
            .find(|p| self.dir.join(p).exists())
            .map(String::from)
    }

    pub fn is_proc_macro(&self) -> bool {
        self.section("lib")
            .and_then(|l| l.get("proc-macro"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Whether Cargo would run a build script for the crate
    pub fn has_build_script(&self) -> bool {
        match self.package("build") {
            Some(Value::Boolean(false)) => false,
            Some(Value::String(path)) => self.dir.join(path).is_file(),
            _ => self.dir.join("build.rs").is_file(),
        }
    }

    /// The CARGO_PKG_* variables Cargo sets, which crates read with env!()
    pub fn cargo_env(&self, crate_name: &str) -> BTreeMap<String, String> {
        let version = self.package_str("version").unwrap_or("0.0.0");
        let (core, pre) = version.split_once('-').unwrap_or((version, ""));
        let mut parts = core.split('.');
        let mut part = || parts.next().unwrap_or("0").to_string();
        let mut env = BTreeMap::from([
            ("CARGO_PKG_NAME".to_string(), crate_name.to_string()),
            ("CARGO_PKG_VERSION".to_string(), version.to_string()),
            ("CARGO_PKG_VERSION_MAJOR".to_string(), part()),
            ("CARGO_PKG_VERSION_MINOR".to_string(), part()),
            ("CARGO_PKG_VERSION_PATCH".to_string(), part()),
            ("CARGO_PKG_VERSION_PRE".to_string(), pre.to_string()),
        ]);
        for (key, var) in [
            ("description", "CARGO_PKG_DESCRIPTION"),
            ("homepage", "CARGO_PKG_HOMEPAGE"),
            ("repository", "CARGO_PKG_REPOSITORY"),
            ("license", "CARGO_PKG_LICENSE"),
        ] {
            if let Some(value) = self.package_str(key) {
                env.insert(var.to_string(), value.to_string());
            }
        }
        match self.package("authors") {
            Some(Value::Array(authors)) => {
                let names: Vec<&str> = authors.iter().filter_map(Value::as_str).collect();
                env.insert("CARGO_PKG_AUTHORS".to_string(), names.join(":"));
            }
            Some(Value::String(author)) => {
                env.insert("CARGO_PKG_AUTHORS".to_string(), author.clone());
            }
            _ => {}
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_dir(manifest: &str, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn reads_the_package() {
        let dir = crate_dir(
            r#"[package]
name = "serde_json"
version = "1.0.149-rc.1"
edition = "2021"
description = "A JSON serialization file format"
authors = ["Erick Tryzelaar <erick.tryzelaar@gmail.com>", "David Tolnay <dtolnay@gmail.com>"]
license = "MIT OR Apache-2.0"
"#,
            &["src/lib.rs", "build.rs"],
        );
        let m = Manifest::read(dir.path()).unwrap();
        assert_eq!(m.crate_name(), "serde_json");
        assert_eq!(m.edition(), "2021");
        assert_eq!(m.lib_path().as_deref(), Some("src/lib.rs"));
        assert!(!m.is_proc_macro());
        assert!(m.has_build_script());
        let env = m.cargo_env("serde_json");
        assert_eq!(env["CARGO_PKG_VERSION_MAJOR"], "1");
        assert_eq!(env["CARGO_PKG_VERSION_PATCH"], "149");
        assert_eq!(env["CARGO_PKG_VERSION_PRE"], "rc.1");
        assert_eq!(
            env["CARGO_PKG_AUTHORS"],
            "Erick Tryzelaar <erick.tryzelaar@gmail.com>:David Tolnay <dtolnay@gmail.com>"
        );
        assert!(!env.contains_key("CARGO_PKG_HOMEPAGE"));
    }

    #[test]
    fn defaults() {
        let dir = crate_dir(
            "[package]\nbuild = false\n[lib]\nproc-macro = true\n",
            &["build.rs"],
        );
        let m = Manifest::read(dir.path()).unwrap();
        assert_eq!(m.edition(), "2015");
        assert_eq!(m.lib_path(), None);
        assert!(m.is_proc_macro());
        assert!(!m.has_build_script());
        assert_eq!(m.cargo_env("x")["CARGO_PKG_VERSION"], "0.0.0");
    }
}
