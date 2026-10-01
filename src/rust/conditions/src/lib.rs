//! conditions: the build configurations a target's deps can depend on, and
//! the `select()` turnkey writes for deps that differ between them
//!
//! A configuration is one value for each of a set of named dimensions, and
//! each value is backed by a Buck2 constraint:
//!
//! - `os`: the platform's operating system (`config//os:linux`, ...)
//! - `cpu`: the platform's CPU (`config//cpu:x86_64`, ...)
//! - on/off dimensions a language adds for a package ([`OnOff`]), e.g. one
//!   per Go build tag that matters, each backed by a constraint's `[set]`
//!   and `[unset]` values
//!
//! The platforms turnkey builds for (the `buck2.platforms` Nix option) fix
//! which os and cpu values go together. Every configuration is evaluated,
//! so what is written never depends on the host it runs on.
//!
//! [`conditional`] turns a label list per configuration into the attribute
//! value written in `rules.star`, and [`json`] decodes the platforms of a
//! JSON configuration as the Go tools did.
//!
//! The Rust port of src/go/pkg/conditions and src/go/pkg/conditional
//! (#212), with the same keys and the same splits: testdata/split-vectors.json
//! holds the test cases src/go/pkg/conditions, turnkey.cfg
//! (src/python/cfg), rust-rules-gen and nix/buck2/platforms.nix run too.

pub mod conditional;
pub mod json;

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// The dimension of a platform's operating system
pub const OS: &str = "os";
/// The dimension of a platform's CPU
pub const CPU: &str = "cpu";

/// An on/off dimension's value when its constraint is set
pub const SET: &str = "set";
/// An on/off dimension's value when its constraint is unset
pub const UNSET: &str = "unset";

/// The Buck2 package holding turnkey's combined config_settings (one per
/// platform, named `<os>-<cpu>`), when none is named
pub const DEFAULT_SETTINGS_PACKAGE: &str = "toolchains//conditions";

/// The `select()` key that matches when no other does. turnkey reads it
/// but never writes it.
pub const DEFAULT_KEY: &str = "DEFAULT";

/// One platform turnkey builds for, in Buck2's constraint names
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize)]
pub struct Platform {
    /// The operating system, a value of `config//os` (e.g. `linux`)
    #[serde(default)]
    pub os: String,
    /// The CPU, a value of `config//cpu` (e.g. `x86_64`)
    #[serde(default)]
    pub cpu: String,
}

impl fmt::Display for Platform {
    /// `<os>-<cpu>`, the name of its combined config_setting
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.os, self.cpu)
    }
}

/// A value for each of a set of dimensions. A dimension it doesn't assign
/// reads as `""`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Configuration(BTreeMap<String, String>);

impl Configuration {
    /// The configuration of the given dimensions' values
    pub fn new<'a>(values: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Configuration(
            values
                .into_iter()
                .map(|(d, v)| (d.to_string(), v.to_string()))
                .collect(),
        )
    }

    /// The value of `dim`, `""` when it has none
    pub fn get(&self, dim: &str) -> &str {
        self.0.get(dim).map(String::as_str).unwrap_or("")
    }

    /// Whether it assigns `dim` a value
    pub fn has(&self, dim: &str) -> bool {
        self.0.contains_key(dim)
    }

    /// Assigns `value` to `dim`
    pub fn set(&mut self, dim: &str, value: &str) {
        self.0.insert(dim.to_string(), value.to_string());
    }

    /// The number of dimensions it assigns
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it assigns no dimension
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The configuration restricted to `dims`
    pub fn project(&self, dims: &[String]) -> Configuration {
        Configuration(
            dims.iter()
                .filter_map(|d| self.0.get(d).map(|v| (d.clone(), v.clone())))
                .collect(),
        )
    }

    /// Whether it assigns every dimension of `partial` the same value
    pub fn includes(&self, partial: &Configuration) -> bool {
        partial.0.iter().all(|(dim, value)| self.get(dim) == value)
    }
}

impl fmt::Display for Configuration {
    /// `dim=value,...` in dimension name order, e.g. `cpu=arm64,os=linux`.
    /// It identifies the configuration.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|(d, v)| format!("{d}={v}")).collect();
        f.write_str(&parts.join(","))
    }
}

/// An on/off dimension a language adds to the space a package's deps are
/// resolved in, e.g. a Go build tag's. Its values are [`SET`] and
/// [`UNSET`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct OnOff {
    /// Identifies the dimension, e.g. `go_tag:integration`
    #[serde(default)]
    pub name: String,
    /// The Buck2 constraint whose `[set]` and `[unset]` values back the
    /// dimension's, e.g. `prelude//go/tags/constraints:integration`
    #[serde(default)]
    pub constraint: String,
    /// Names [`SET`] in a combined config_setting's name, e.g.
    /// `integration`; [`UNSET`] is `no_<token>`
    #[serde(default)]
    pub token: String,
}

/// How a dimension's values are written
#[derive(Debug, Clone, PartialEq, Eq)]
enum Backing {
    /// `config//<name>:<value>`, the value itself in combined names
    Platform,
    /// `<constraint>[<value>]`, `<token>` or `no_<token>` in combined names
    OnOff { constraint: String, token: String },
}

/// A named axis of the configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dimension {
    /// Identifies the dimension, e.g. `os`
    pub name: String,
    /// The dimension's values, in the order keys are written
    pub values: Vec<String>,
    backing: Backing,
}

impl Dimension {
    /// The `select()` key that matches `value` alone, e.g.
    /// `config//os:linux`
    pub fn key(&self, value: &str) -> String {
        match &self.backing {
            Backing::Platform => format!("config//{}:{value}", self.name),
            Backing::OnOff { constraint, .. } => format!("{constraint}[{value}]"),
        }
    }

    /// The name of `value` in a combined config_setting's name
    fn token(&self, value: &str) -> String {
        match &self.backing {
            Backing::Platform => value.to_string(),
            Backing::OnOff { token, .. } if value == SET => token.clone(),
            Backing::OnOff { token, .. } => format!("no_{token}"),
        }
    }
}

/// The set of configurations evaluated: one per platform, crossed with
/// each on/off dimension's values
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    /// The space's dimensions, in the order preferred for `select()` keys
    pub dimensions: Vec<Dimension>,
    /// The space's configurations
    pub configurations: Vec<Configuration>,
    /// The Buck2 package of the combined config_settings
    settings: String,
}

impl Space {
    /// The space of the given platforms. `settings` is the Buck2 package
    /// holding one combined config_setting per platform
    /// ([`DEFAULT_SETTINGS_PACKAGE`] when empty). With no platforms, the
    /// space has a single configuration with no dimensions: deps can't
    /// depend on it.
    pub fn new(platforms: &[Platform], settings: &str) -> Space {
        let settings = if settings.is_empty() {
            DEFAULT_SETTINGS_PACKAGE
        } else {
            settings
        };
        let mut space = Space {
            dimensions: Vec::new(),
            configurations: Vec::new(),
            settings: settings.to_string(),
        };
        if platforms.is_empty() {
            space.configurations.push(Configuration::default());
            return space;
        }

        let (mut oses, mut cpus) = (Vec::new(), Vec::new());
        let mut seen = HashSet::new();
        for p in platforms {
            let config = Configuration::new([(OS, p.os.as_str()), (CPU, p.cpu.as_str())]);
            // Platforms are told apart by their configuration's name, as
            // the Go version does
            if !seen.insert(config.to_string()) {
                continue;
            }
            space.configurations.push(config);
            append_new(&mut oses, &p.os);
            append_new(&mut cpus, &p.cpu);
        }
        oses.sort();
        cpus.sort();
        space.dimensions = vec![
            Dimension {
                name: OS.to_string(),
                values: oses,
                backing: Backing::Platform,
            },
            Dimension {
                name: CPU.to_string(),
                values: cpus,
                backing: Backing::Platform,
            },
        ];
        space
    }

    /// The space with the on/off dimensions `dims` added, sorted by name,
    /// each crossing every configuration with both its values. Names
    /// already in the space are ignored.
    pub fn with_dimensions(&self, dims: &[OnOff]) -> Space {
        let mut added: Vec<&OnOff> = Vec::new();
        for d in dims {
            if self.dimension(&d.name).is_none() && !added.iter().any(|a| a.name == d.name) {
                added.push(d);
            }
        }
        if added.is_empty() {
            return self.clone();
        }
        added.sort_by(|a, b| a.name.cmp(&b.name));

        let mut extended = Space {
            dimensions: self.dimensions.clone(),
            configurations: Vec::new(),
            settings: self.settings.clone(),
        };
        let mut configs = self.configurations.clone();
        for d in added {
            extended.dimensions.push(Dimension {
                name: d.name.clone(),
                values: vec![SET.to_string(), UNSET.to_string()],
                backing: Backing::OnOff {
                    constraint: d.constraint.clone(),
                    token: d.token.clone(),
                },
            });
            let mut crossed = Vec::with_capacity(configs.len() * 2);
            for config in &configs {
                for v in [SET, UNSET] {
                    let mut c = config.clone();
                    c.set(&d.name, v);
                    crossed.push(c);
                }
            }
            configs = crossed;
        }
        extended.configurations = configs;
        extended
    }

    /// The space's dimension named `name`
    fn dimension(&self, name: &str) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.name == name)
    }

    /// The `select()` key matching the configurations that assign
    /// `partial`'s values: the dimension's own key for a single dimension,
    /// or the combined config_setting for several
    fn key(&self, dims: &[String], partial: &Configuration) -> String {
        if let [dim] = dims {
            let d = self.dimension(dim).expect("a dimension of the space");
            return d.key(partial.get(dim));
        }
        let tokens: Vec<String> = dims
            .iter()
            .map(|dim| {
                let d = self.dimension(dim).expect("a dimension of the space");
                d.token(partial.get(dim))
            })
            .collect();
        format!("{}:{}", self.settings, tokens.join("-"))
    }

    /// Every `select()` key the space can write, with the partial
    /// configuration each one matches
    fn keys(&self) -> HashMap<String, Configuration> {
        let mut keys = HashMap::new();
        for dims in self.dimension_sets() {
            for config in &self.configurations {
                let partial = config.project(&dims);
                keys.insert(self.key(&dims, &partial), partial);
            }
        }
        keys
    }

    /// Every non-empty set of the space's dimensions, by increasing size,
    /// each in the order of `dimensions`. The first one that explains a
    /// difference between configurations gives the smallest keys.
    fn dimension_sets(&self) -> Vec<Vec<String>> {
        fn pick(
            names: &[String],
            size: usize,
            start: usize,
            set: &mut Vec<String>,
            sets: &mut Vec<Vec<String>>,
        ) {
            if set.len() == size {
                sets.push(set.clone());
                return;
            }
            for i in start..names.len() {
                set.push(names[i].clone());
                pick(names, size, i + 1, set, sets);
                set.pop();
            }
        }
        let names: Vec<String> = self.dimensions.iter().map(|d| d.name.clone()).collect();
        let mut sets = Vec::new();
        for size in 1..=names.len() {
            pick(&names, size, 0, &mut Vec::new(), &mut sets);
        }
        sets
    }

    /// Splits each configuration's labels into common and conditional
    /// ones. Common labels keep the order of the first configuration; a
    /// branch's labels keep the order of its first configuration. Keys are
    /// the smallest exact ones: the fewest dimensions (preferring the
    /// space's order) whose values determine each configuration's extra
    /// labels.
    pub fn split(&self, mut labels: impl FnMut(&Configuration) -> Vec<String>) -> Split {
        let per: Vec<Vec<String>> = self
            .configurations
            .iter()
            .map(|config| dedupe(labels(config)))
            .collect();

        let common: Vec<String> = per[0]
            .iter()
            .filter(|label| per[1..].iter().all(|other| other.contains(label)))
            .cloned()
            .collect();

        let extra: Vec<Vec<String>> = per
            .iter()
            .map(|list| {
                list.iter()
                    .filter(|label| !common.contains(label))
                    .cloned()
                    .collect()
            })
            .collect();
        if extra.iter().all(Vec::is_empty) {
            return Split {
                common,
                branches: Vec::new(),
            };
        }

        for dims in self.dimension_sets() {
            if let Some(branches) = self.branches(&dims, &extra) {
                return Split { common, branches };
            }
        }
        // Every configuration is distinct in the full set of dimensions,
        // so the last set always explains the differences.
        panic!("conditions: no dimension set explains the configurations' differences");
    }

    /// Keys each configuration's extra labels by its values of `dims`.
    /// `None` if two configurations with the same values have different
    /// extras: `dims` don't explain the difference.
    fn branches(&self, dims: &[String], extra: &[Vec<String>]) -> Option<Vec<Branch>> {
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut branches: Vec<Branch> = Vec::new();
        for (i, config) in self.configurations.iter().enumerate() {
            let key = self.key(dims, &config.project(dims));
            if let Some(&j) = by_key.get(&key) {
                if !same_set(&branches[j].labels, &extra[i]) {
                    return None;
                }
                continue;
            }
            by_key.insert(key.clone(), branches.len());
            branches.push(Branch {
                key,
                labels: extra[i].clone(),
            });
        }
        branches.sort_by(|a, b| a.key.cmp(&b.key));
        Some(branches)
    }

    /// The matcher of a `select()` with `keys`, or an error if a key is
    /// neither [`DEFAULT_KEY`] nor one the space writes: which
    /// configurations it matches can't be told.
    pub fn matcher(&self, keys: &[&str]) -> Result<Matcher, UnknownKey> {
        let known = self.keys();
        let mut matches = Vec::with_capacity(keys.len());
        for key in keys {
            if *key == DEFAULT_KEY {
                matches.push(None);
                continue;
            }
            match known.get(*key) {
                Some(partial) => matches.push(Some(partial.clone())),
                None => return Err(UnknownKey(key.to_string())),
            }
        }
        Ok(Matcher { matches })
    }
}

/// A `select()` key that is neither `DEFAULT` nor one the space writes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKey(pub String);

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "select() key {} is not one sync knows",
            gostd::strconv::quote(&self.0)
        )
    }
}

impl std::error::Error for UnknownKey {}

/// One `select()` entry: the labels a key adds
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branch {
    /// The `select()` key, e.g. `config//os:linux`
    pub key: String,
    /// The labels it adds to the common ones
    pub labels: Vec<String>,
}

/// A label list over a space: the labels every configuration has, and per
/// `select()` key the labels only some have. It is written as
/// `[<common>] + select({<key>: [<labels>], ...})`, with no `DEFAULT`
/// branch, or as a plain list when there are no branches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Split {
    /// The labels of every configuration
    pub common: Vec<String>,
    /// The `select()` entries, sorted by key; empty when every
    /// configuration has the same labels
    pub branches: Vec<Branch>,
}

impl Split {
    /// Whether the split needs a `select()`
    pub fn is_conditional(&self) -> bool {
        !self.branches.is_empty()
    }
}

/// Tells which of a `select()`'s keys matches each configuration of a
/// space
#[derive(Debug, Clone)]
pub struct Matcher {
    /// Per key, the partial configuration it matches; `None` for `DEFAULT`
    matches: Vec<Option<Configuration>>,
}

impl Matcher {
    /// The index of the key that applies in `config`: the most specific
    /// one matching it, or `DEFAULT` if none does. `None` if no key
    /// applies (the build would fail in `config`).
    pub fn branch(&self, config: &Configuration) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        let mut default = None;
        for (i, partial) in self.matches.iter().enumerate() {
            match partial {
                None => default = Some(i),
                Some(partial) => {
                    if config.includes(partial) && best.is_none_or(|(_, size)| partial.len() > size)
                    {
                        best = Some((i, partial.len()));
                    }
                }
            }
        }
        best.map(|(i, _)| i).or(default)
    }
}

/// Appends `s` to `list` unless it is already there
fn append_new(list: &mut Vec<String>, s: &str) {
    if !list.iter().any(|v| v == s) {
        list.push(s.to_string());
    }
}

/// `list` without repeated labels, in order
pub(crate) fn dedupe(list: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    list.into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

/// Whether `a` and `b` hold the same labels, in any order
fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|s| b.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// turnkey's default platforms
    fn platforms() -> Vec<Platform> {
        [
            ("linux", "x86_64"),
            ("linux", "arm64"),
            ("macos", "x86_64"),
            ("macos", "arm64"),
        ]
        .into_iter()
        .map(|(os, cpu)| Platform {
            os: os.into(),
            cpu: cpu.into(),
        })
        .collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn branch(key: &str, labels: &[&str]) -> Branch {
        Branch {
            key: key.into(),
            labels: strings(labels),
        }
    }

    /// A labels function giving each configuration the common labels plus
    /// those of the first matching entry of extra, keyed by `os=...`,
    /// `cpu=...` or `cpu=...,os=...`
    fn labels_by<'a>(
        common: &'a [&'a str],
        extra: &'a [(&'a str, &'a [&'a str])],
    ) -> impl Fn(&Configuration) -> Vec<String> + 'a {
        move |c| {
            let mut labels = strings(common);
            for key in [
                c.to_string(),
                format!("os={}", c.get(OS)),
                format!("cpu={}", c.get(CPU)),
            ] {
                if let Some((_, e)) = extra.iter().find(|(k, _)| *k == key) {
                    labels.extend(strings(e));
                    return labels;
                }
            }
            labels
        }
    }

    #[test]
    fn identical_everywhere_is_a_plain_list() {
        let got = Space::new(&platforms(), "").split(labels_by(&["//a:a", "//b:b"], &[]));
        assert_eq!(
            got,
            Split {
                common: strings(&["//a:a", "//b:b"]),
                branches: vec![]
            }
        );
        assert!(!got.is_conditional(), "identical deps need no select()");
    }

    #[test]
    fn os_only_differences_use_os_keys() {
        let got = Space::new(&platforms(), "").split(labels_by(
            &["//unix:unix"],
            &[
                ("os=linux", &["//linux:only"]),
                ("os=macos", &["//macos:only"]),
            ],
        ));
        assert_eq!(
            got,
            Split {
                common: strings(&["//unix:unix"]),
                branches: vec![
                    branch("config//os:linux", &["//linux:only"]),
                    branch("config//os:macos", &["//macos:only"]),
                ],
            }
        );
    }

    /// An OS with no extra deps still gets its branch: there is no DEFAULT.
    #[test]
    fn empty_branch_for_other_os() {
        let got =
            Space::new(&platforms(), "").split(labels_by(&[], &[("os=linux", &["//linux:only"])]));
        assert_eq!(
            got.branches,
            vec![
                branch("config//os:linux", &["//linux:only"]),
                branch("config//os:macos", &[]),
            ]
        );
    }

    #[test]
    fn cpu_difference_within_one_os_uses_combined_key() {
        let got = Space::new(&platforms(), "").split(labels_by(
            &[],
            &[("cpu=arm64,os=linux", &["//linux-arm:only"])],
        ));
        assert_eq!(
            got.branches,
            vec![
                branch("toolchains//conditions:linux-arm64", &["//linux-arm:only"]),
                branch("toolchains//conditions:linux-x86_64", &[]),
                branch("toolchains//conditions:macos-arm64", &[]),
                branch("toolchains//conditions:macos-x86_64", &[]),
            ]
        );
    }

    #[test]
    fn cpu_only_differences_use_cpu_keys() {
        let got =
            Space::new(&platforms(), "").split(labels_by(&[], &[("cpu=x86_64", &["//x86:only"])]));
        assert_eq!(
            got.branches,
            vec![
                branch("config//cpu:arm64", &[]),
                branch("config//cpu:x86_64", &["//x86:only"]),
            ]
        );
    }

    #[test]
    fn never_writes_default() {
        let space = Space::new(&platforms(), "");
        let cases: [&[(&str, &[&str])]; 4] = [
            &[("os=linux", &["//l:l"])],
            &[("cpu=arm64", &["//a:a"])],
            &[("cpu=arm64,os=macos", &["//m:m"])],
            &[("os=linux", &["//l:l"]), ("os=macos", &["//m:m"])],
        ];
        for extra in cases {
            for b in space.split(labels_by(&["//c:c"], extra)).branches {
                assert!(
                    b.key != DEFAULT_KEY && !b.key.contains("DEFAULT"),
                    "split of {extra:?} wrote a DEFAULT branch"
                );
            }
        }
    }

    fn config(os: &str, cpu: &str) -> Configuration {
        Configuration::new([(OS, os), (CPU, cpu)])
    }

    #[test]
    fn matcher_default_and_unknown_keys() {
        let space = Space::new(&platforms(), "");
        let m = space.matcher(&["config//os:linux", DEFAULT_KEY]).unwrap();
        assert_eq!(m.branch(&config("macos", "arm64")), Some(1));
        assert_eq!(m.branch(&config("linux", "arm64")), Some(0));
        assert!(
            space.matcher(&["//my:setting"]).is_err(),
            "an unknown key was matched"
        );
    }

    /// With no key matching and no DEFAULT, no branch applies.
    #[test]
    fn matcher_without_a_branch() {
        let m = Space::new(&platforms(), "")
            .matcher(&["config//os:linux"])
            .unwrap();
        assert_eq!(m.branch(&config("macos", "arm64")), None);
    }

    /// The combined key wins over an OS key when both match.
    #[test]
    fn matcher_prefers_the_most_specific_key() {
        let m = Space::new(&platforms(), "")
            .matcher(&["config//os:linux", "toolchains//conditions:linux-arm64"])
            .unwrap();
        assert_eq!(m.branch(&config("linux", "arm64")), Some(1));
    }

    /// With no platforms there is one configuration, and deps can't differ.
    #[test]
    fn space_without_platforms() {
        let space = Space::new(&[], "");
        assert_eq!(space.configurations, vec![Configuration::default()]);
        assert!(!space.split(labels_by(&["//a:a"], &[])).is_conditional());
    }

    /// A Go build tag's on/off dimension, as the Go plug-in adds it
    fn integration() -> OnOff {
        OnOff {
            name: "go_tag:integration".into(),
            constraint: "prelude//go/tags/constraints:integration".into(),
            token: "integration".into(),
        }
    }

    /// An on/off dimension: deps that differ only by it are keyed on its
    /// constraint values, and with the OS on a combined setting. Adding it
    /// twice, or the OS, adds nothing.
    #[test]
    fn split_on_on_off_dimension() {
        let os = OnOff {
            name: OS.into(),
            ..OnOff::default()
        };
        let space =
            Space::new(&platforms(), "").with_dimensions(&[integration(), integration(), os]);
        assert_eq!(space.configurations.len(), 8);
        let name = integration().name;
        let got = space.split(|c| {
            if c.get(&name) == SET {
                strings(&["//it:it"])
            } else {
                vec![]
            }
        });
        assert_eq!(
            got.branches,
            vec![
                branch(
                    "prelude//go/tags/constraints:integration[set]",
                    &["//it:it"]
                ),
                branch("prelude//go/tags/constraints:integration[unset]", &[]),
            ]
        );

        let got = space.split(|c| {
            if c.get(OS) == "linux" && c.get(&name) == SET {
                strings(&["//it:it"])
            } else {
                vec![]
            }
        });
        let keys: Vec<&str> = got.branches.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "toolchains//conditions:linux-integration",
                "toolchains//conditions:linux-no_integration",
                "toolchains//conditions:macos-integration",
                "toolchains//conditions:macos-no_integration",
            ]
        );
        assert_eq!(got.branches[0].labels, ["//it:it"]);
    }

    #[test]
    fn configuration_names() {
        assert_eq!(config("linux", "arm64").to_string(), "cpu=arm64,os=linux");
        assert_eq!(Configuration::default().to_string(), "");
        assert_eq!(
            Platform {
                os: "macos".into(),
                cpu: "arm64".into()
            }
            .to_string(),
            "macos-arm64"
        );
    }

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Vector>,
    }

    #[derive(Deserialize)]
    struct Vector {
        name: String,
        platforms: Vec<Platform>,
        settings: String,
        dimensions: Vec<OnOff>,
        labels: Vec<Rule>,
        common: Vec<String>,
        branches: Vec<Branch2>,
    }

    #[derive(Deserialize)]
    struct Rule {
        when: Configuration,
        labels: Vec<String>,
    }

    #[derive(Deserialize)]
    struct Branch2 {
        key: String,
        labels: Vec<String>,
    }

    /// The test cases src/go/pkg/conditions, turnkey.cfg, rust-rules-gen
    /// and nix/buck2/platforms.nix run too. testdata/ links to the file,
    /// and Buck2 maps it to the same path.
    #[test]
    fn split_vectors() {
        let vectors: Vectors =
            serde_json::from_str(include_str!("../testdata/split-vectors.json")).unwrap();
        for c in vectors.cases {
            let space = Space::new(&c.platforms, &c.settings).with_dimensions(&c.dimensions);
            let got = space.split(|config| {
                c.labels
                    .iter()
                    .filter(|rule| config.includes(&rule.when))
                    .flat_map(|rule| rule.labels.clone())
                    .collect()
            });
            assert_eq!(got.common, c.common, "{}: common", c.name);
            let want: Vec<Branch> = c
                .branches
                .iter()
                .map(|b| Branch {
                    key: b.key.clone(),
                    labels: b.labels.clone(),
                })
                .collect();
            assert_eq!(got.branches, want, "{}: branches", c.name);
        }
    }
}
