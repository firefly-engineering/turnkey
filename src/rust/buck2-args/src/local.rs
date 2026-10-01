//! Local target overrides: `.turnkey/local.toml`
//!
//! A file not committed to git, where each developer gives a target the
//! arguments tk injects after `--` when it runs, builds or tests it
//! (different network addresses, debug flags or local ports):
//!
//! ```toml
//! [run."//docs/user-manual"]
//! args = ["-n", "100.64.25.26"]
//!
//! [build."//some:target"]
//! args = ["--config=debug"]
//!
//! [test."//src/pkg/..."]
//! args = ["--test-arg=foo"]
//! ```
//!
//! The file is decoded as go-toml decoded it into the Go version's struct:
//! a key names a field when the two are equal but for ASCII case, unknown
//! keys are ignored, and a value of the wrong type is an error.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// Where the local config file is, relative to the project root
pub const DEFAULT_CONFIG_PATH: &str = ".turnkey/local.toml";

/// The local override configuration
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalConfig {
    /// Per-target argument overrides for `tk run`
    pub run: BTreeMap<String, TargetOverride>,
    /// Per-target argument overrides for `tk build`
    pub build: BTreeMap<String, TargetOverride>,
    /// Per-target argument overrides for `tk test`
    pub test: BTreeMap<String, TargetOverride>,
}

/// The overrides for one target (or pattern)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetOverride {
    /// The arguments injected after `--` for this target
    pub args: Vec<String>,
}

/// Why the local config couldn't be loaded
#[derive(Debug)]
pub enum Error {
    /// The file couldn't be read
    Read(std::io::Error),
    /// The file isn't a valid local config
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(e) => write!(f, "failed to read local config: {e}"),
            Error::Parse(e) => write!(f, "failed to parse local config: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl LocalConfig {
    /// Reads the config file at `path`
    pub fn load(path: &Path) -> Result<Self, Error> {
        let data = std::fs::read(path).map_err(Error::Read)?;
        Self::parse(&data)
    }

    /// Reads the config from [`DEFAULT_CONFIG_PATH`] under `root`. A file
    /// that doesn't exist is an empty config, not an error.
    pub fn load_default_from(root: &Path) -> Result<Self, Error> {
        let path = root.join(DEFAULT_CONFIG_PATH);
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            _ => Self::load(&path),
        }
    }

    /// Parses the config from TOML data
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(data).map_err(|e| Error::Parse(e.to_string()))?;
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| Error::Parse(e.message().to_string()))?;
        let mut config = Self::default();
        for (key, value) in table {
            let overrides = if key.eq_ignore_ascii_case("run") {
                &mut config.run
            } else if key.eq_ignore_ascii_case("build") {
                &mut config.build
            } else if key.eq_ignore_ascii_case("test") {
                &mut config.test
            } else {
                continue;
            };
            decode_overrides(&key, value, overrides)?;
        }
        Ok(config)
    }

    /// The override for `command`'s `target`: the target's own, or else
    /// that of the longest pattern matching it ([`match_target`]), and of
    /// two as long, the first in byte order. `None` for a command other
    /// than `run`, `build` or `test`.
    pub fn get_override(&self, command: &str, target: &str) -> Option<&TargetOverride> {
        let overrides = match command {
            "run" => &self.run,
            "build" => &self.build,
            "test" => &self.test,
            _ => return None,
        };
        if let Some(found) = overrides.get(target) {
            return Some(found);
        }
        // In byte order: the first of the longest is the one kept
        let mut best: Option<(&String, &TargetOverride)> = None;
        for (pattern, found) in overrides {
            if match_target(pattern, target) && best.is_none_or(|(b, _)| pattern.len() > b.len()) {
                best = Some((pattern, found));
            }
        }
        best.map(|(_, found)| found)
    }

    /// Whether any override is configured
    pub fn has_overrides(&self) -> bool {
        !self.run.is_empty() || !self.build.is_empty() || !self.test.is_empty()
    }
}

/// Decodes a command's table of overrides into `overrides`
fn decode_overrides(
    command: &str,
    value: toml::Value,
    overrides: &mut BTreeMap<String, TargetOverride>,
) -> Result<(), Error> {
    let toml::Value::Table(table) = value else {
        return Err(type_error(command, &value, "a table of targets"));
    };
    for (target, value) in table {
        let toml::Value::Table(fields) = value else {
            return Err(type_error(&target, &value, "a table"));
        };
        let entry = overrides.entry(target.clone()).or_default();
        for (key, value) in fields {
            if !key.eq_ignore_ascii_case("args") {
                continue;
            }
            let toml::Value::Array(items) = value else {
                return Err(type_error(&key, &value, "an array of strings"));
            };
            entry.args = items
                .into_iter()
                .map(|item| match item {
                    toml::Value::String(s) => Ok(s),
                    other => Err(type_error(&key, &other, "a string")),
                })
                .collect::<Result<_, _>>()?;
        }
    }
    Ok(())
}

fn type_error(key: &str, value: &toml::Value, want: &str) -> Error {
    Error::Parse(format!(
        "{key}: cannot decode a TOML {} into {want}",
        value.type_str()
    ))
}

/// Whether `target` matches `pattern`: the target itself, or, for a
/// pattern ending in `...`, any target the rest of it starts. `//foo/...`
/// matches `//foo/sub:baz`, and also the package's own targets, `//foo:bar`;
/// `//...` matches every target.
pub fn match_target(pattern: &str, target: &str) -> bool {
    if pattern == target {
        return true;
    }
    let Some(prefix) = pattern.strip_suffix("...") else {
        return false;
    };
    if target.starts_with(prefix) {
        return true;
    }
    // //foo/... matches //foo:bar too
    match prefix.strip_suffix('/') {
        Some(package) => target
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with(':')),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> LocalConfig {
        LocalConfig::parse(toml.as_bytes()).unwrap()
    }

    #[test]
    fn empty_config() {
        let config = parse("");
        assert!(config.run.is_empty() && config.build.is_empty() && config.test.is_empty());
        assert!(!config.has_overrides());
    }

    #[test]
    fn run_override() {
        let config = parse(
            r#"
[run."//docs/user-manual"]
args = ["-n", "100.64.25.26"]
"#,
        );
        let found = config.get_override("run", "//docs/user-manual").unwrap();
        assert_eq!(found.args, ["-n", "100.64.25.26"]);
    }

    #[test]
    fn multiple_commands() {
        let config = parse(
            r#"
[run."//target:a"]
args = ["--run-flag"]

[build."//target:b"]
args = ["--build-flag"]

[test."//target:c"]
args = ["--test-flag"]
"#,
        );
        assert_eq!(
            (config.run.len(), config.build.len(), config.test.len()),
            (1, 1, 1)
        );
    }

    #[test]
    fn pattern_match() {
        let config = parse(
            r#"
[test."//src/pkg/..."]
args = ["--verbose"]
"#,
        );
        let found = config.get_override("test", "//src/pkg/foo:bar").unwrap();
        assert_eq!(found.args, ["--verbose"]);
    }

    #[test]
    fn keys_are_matched_whatever_their_case_and_unknown_ones_ignored() {
        let config = parse(
            r#"
other = 1

[Run."//a:b"]
ARGS = ["x"]
unknown = true
"#,
        );
        assert_eq!(config.get_override("run", "//a:b").unwrap().args, ["x"]);
    }

    #[test]
    fn values_of_the_wrong_type_are_errors() {
        for bad in [
            "run = 1",
            "[run]\n\"//a:b\" = 1",
            "[run.\"//a:b\"]\nargs = \"x\"",
            "[run.\"//a:b\"]\nargs = [1]",
            "not toml",
        ] {
            assert!(LocalConfig::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn match_target_cases() {
        for (pattern, target, want) in [
            ("//foo:bar", "//foo:bar", true),
            ("//foo:bar", "//foo:baz", false),
            ("//foo/...", "//foo/bar:baz", true),
            ("//foo/...", "//foo:bar", true),
            ("//foo/...", "//bar:baz", false),
            ("//...", "//anything:here", true),
        ] {
            assert_eq!(match_target(pattern, target), want, "{pattern} {target}");
        }
    }

    fn overrides(entries: &[(&str, &[&str])]) -> BTreeMap<String, TargetOverride> {
        entries
            .iter()
            .map(|(target, args)| {
                (
                    target.to_string(),
                    TargetOverride {
                        args: args.iter().map(|a| a.to_string()).collect(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn get_override() {
        let config = LocalConfig {
            run: overrides(&[("//docs/user-manual", &["-n", "localhost"])]),
            build: overrides(&[("//src/...", &["--debug"])]),
            test: BTreeMap::new(),
        };
        assert!(config.get_override("run", "//docs/user-manual").is_some());
        assert!(config.get_override("build", "//src/pkg:foo").is_some());
        assert!(config.get_override("run", "//other:target").is_none());
        assert!(
            config
                .get_override("install", "//docs/user-manual")
                .is_none()
        );
    }

    #[test]
    fn the_most_specific_pattern_wins() {
        let config = LocalConfig {
            test: overrides(&[
                ("//...", &["everything"]),
                ("//src/...", &["src"]),
                ("//src/pkg/...", &["pkg"]),
                ("//src/pkg:...", &["pkg-colon"]),
                ("//src/other/...", &["other"]),
            ]),
            ..Default::default()
        };
        for (target, want) in [
            ("//src/pkg/sub:t", "pkg"),
            // //src/pkg/... and //src/pkg:... both match, and are as long
            ("//src/pkg:t", "pkg"),
            ("//src/lib:t", "src"),
            ("//docs:t", "everything"),
        ] {
            let found = config.get_override("test", target).unwrap();
            assert_eq!(found.args, [want], "{target}");
        }
    }

    #[test]
    fn has_overrides() {
        let config = LocalConfig {
            run: overrides(&[("//foo:bar", &["--flag"])]),
            ..Default::default()
        };
        assert!(config.has_overrides());
        assert!(!LocalConfig::default().has_overrides());
    }

    #[test]
    fn a_missing_file_is_an_empty_config() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            LocalConfig::load_default_from(root.path()).unwrap(),
            LocalConfig::default()
        );
        std::fs::create_dir(root.path().join(".turnkey")).unwrap();
        std::fs::write(
            root.path().join(DEFAULT_CONFIG_PATH),
            "[build.\"//a:b\"]\nargs = [\"x\"]\n",
        )
        .unwrap();
        let config = LocalConfig::load_default_from(root.path()).unwrap();
        assert_eq!(config.get_override("build", "//a:b").unwrap().args, ["x"]);
    }
}
