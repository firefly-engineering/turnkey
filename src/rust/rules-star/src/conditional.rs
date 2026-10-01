//! A conditional attribute read and written: a target attribute whose
//! value depends on the build configuration, written as a plain value or
//! as `[<labels>] + select({<key>: <value>, ...})` over a [`Space`]
//!
//! It decides what such a value is in each configuration, so rules sync
//! and the mapper's plug-ins agree on it; how a value per configuration is
//! written is [`conditions::conditional`]'s, which the cell generators
//! share. The Rust port of src/go/pkg/conditional's Read, ReadLabels and
//! SetLabels.

use crate::{Error, SelectBranch, SelectValue, Target, Value, labels};
use conditions::{Configuration, Matcher, Space, Split};
use std::collections::HashSet;

/// An attribute read as its value in each configuration of a space (see
/// [`read`])
#[derive(Debug, Clone)]
pub struct Reading(Read);

#[derive(Debug, Clone)]
enum Read {
    /// The same value in every configuration (`None`: absent)
    Fixed(Option<Value>),
    /// A `select()`: its plain part's labels, and which branch applies
    Select {
        sel: SelectValue,
        common: Vec<String>,
        matcher: Matcher,
    },
}

impl Reading {
    /// The attribute's value in `config`
    pub fn value(&self, config: &Configuration) -> Option<Value> {
        let (sel, common, matcher) = match &self.0 {
            Read::Fixed(value) => return value.clone(),
            Read::Select {
                sel,
                common,
                matcher,
            } => (sel, common, matcher),
        };
        let branch = matcher.branch(config).map(|i| &sel.branches[i].value);
        let Some(extra) = labels(branch) else {
            return branch.cloned();
        };
        if sel.common.is_none() && branch.is_none() {
            return None;
        }
        let mut all = common.clone();
        all.extend(extra);
        Some(Value::StringList(dedupe(all)))
    }
}

/// Reads target's attribute `attr` as its value in each configuration of
/// `space`:
///
/// - an absent attribute is `None` in every configuration;
/// - a value that isn't a `select()` is itself in every configuration;
/// - a `select()`'s value is its plain part followed by the value of the
///   branch that applies ([`Matcher::branch`]: the most specific key
///   matching, else `DEFAULT`). When both are label lists, it is a list
///   holding each label once; a `select()` alone whose branch is another
///   kind of value (`True`, a string) gives that value;
/// - a configuration no branch applies to, in which the build would fail,
///   gets the plain part alone: its labels, or `None` for a `select()`
///   alone.
///
/// It is an error if the `select()` can't be read: a key the space doesn't
/// know (neither `DEFAULT` nor one sync writes), or a plain part followed
/// by a branch that isn't a list of labels.
pub fn read(target: &Target, attr: &str, space: &Space) -> Result<Reading, Error> {
    let Some(a) = target.get_attribute(attr) else {
        return Ok(Reading(Read::Fixed(None)));
    };
    let Value::Select(sel) = &a.value else {
        return Ok(Reading(Read::Fixed(Some(a.value.clone()))));
    };
    let Some(common) = labels(sel.common.as_deref()) else {
        return Err(Error(format!(
            "{attr}: the part before select() is not a list of labels"
        )));
    };
    let mut keys = Vec::with_capacity(sel.branches.len());
    for b in &sel.branches {
        keys.push(b.key.as_str());
        if sel.common.is_some() && labels(Some(&b.value)).is_none() {
            return Err(Error(format!(
                "{attr}: select() branch {} is not a list of labels",
                gostd::strconv::quote(&b.key)
            )));
        }
    }
    let matcher = space
        .matcher(&keys)
        .map_err(|err| Error(format!("{attr}: {err}")))?;
    Ok(Reading(Read::Select {
        sel: sel.clone(),
        common,
        matcher,
    }))
}

/// A label-list attribute read as its labels in each configuration (see
/// [`read_labels`])
#[derive(Debug, Clone)]
pub struct LabelsReading(Reading);

impl LabelsReading {
    /// The attribute's labels in `config`
    pub fn labels(&self, config: &Configuration) -> Vec<String> {
        labels(self.0.value(config).as_ref()).unwrap_or_default()
    }
}

/// Reads target's label-list attribute `attr` (deps, npm_deps, ...) as the
/// labels it has in each configuration of `space`, as [`read`] does. An
/// absent attribute has no labels. It is an error if the attribute isn't a
/// list of labels, or a `select()` [`read`] can read whose branches all
/// are: a computed value (a variable, a concatenation, a conditional) isn't
/// read, since whoever writes back would replace the expression with
/// values of its own.
pub fn read_labels(target: &Target, attr: &str, space: &Space) -> Result<LabelsReading, Error> {
    if let Some(a) = target.get_attribute(attr)
        && !is_labels(&a.value)
    {
        return Err(Error(format!(
            "{attr}: {} is not a list of labels",
            describe(&a.value)
        )));
    }
    Ok(LabelsReading(read(target, attr, space)?))
}

/// Whether a value is a list of labels in every configuration: a list, or
/// a `select()` of lists.
fn is_labels(value: &Value) -> bool {
    let Value::Select(sel) = value else {
        return labels(Some(value)).is_some();
    };
    labels(sel.common.as_deref()).is_some()
        && sel
            .branches
            .iter()
            .all(|b| labels(Some(&b.value)).is_some())
}

/// A value as the Go version's `String()` describes it in errors
fn describe(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::StringList(_) | Value::Deps(_) => "[...]".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(_) => String::new(),
        Value::Ident(name) => name.clone(),
        Value::Select(_) => "[...] + select({...})".to_string(),
        Value::Expr(text) => text.clone(),
    }
}

/// Sets target's label-list attribute `attr` to have `labels(config)` in
/// each configuration of `space`: the labels every configuration has, as a
/// plain list, followed by a `select()` of the others when they differ,
/// keyed as [`Space::split`] keys them, or `[]` when no configuration has a
/// label. The markers of the attribute's plain part are kept: the plain
/// part becomes its auto-managed section (see [`Target::set_select`]).
pub fn set_labels(
    target: &mut Target,
    attr: &str,
    space: &Space,
    labels: impl FnMut(&Configuration) -> Vec<String>,
) {
    let split = space.split(labels);
    target.set_select(attr, split.common.clone(), branches(&split));
}

/// A split's `select()` entries, as written
fn branches(split: &Split) -> Vec<SelectBranch> {
    split
        .branches
        .iter()
        .map(|b| SelectBranch {
            key: b.key.clone(),
            value: Value::StringList(b.labels.clone()),
        })
        .collect()
}

/// `list` without repeated labels, in order
fn dedupe(list: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    list.into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{File, parse};
    use conditions::conditional::labels_value;
    use conditions::{CPU, OS, OnOff, Platform, SET};

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

    fn config(os: &str, cpu: &str) -> Configuration {
        Configuration::new([(OS, os), (CPU, cpu)])
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A rules.star of one rule with one attribute, deps, written as
    /// `value`
    fn file(value: &str) -> File {
        let mut src = "rust_library(\n    name = \"lib\",\n".to_string();
        if !value.is_empty() {
            src += &format!("    deps = {value},\n");
        }
        parse("rules.star", src + ")\n").unwrap()
    }

    /// The rule of [`file`]
    fn target(value: &str) -> Target {
        file(value).targets.remove(0)
    }

    type Labels = fn(&Configuration) -> Vec<String>;

    /// Values per configuration to write and read back
    fn labels_cases() -> Vec<(&'static str, Labels)> {
        vec![
            ("none", |_| vec![]),
            ("identical", |_| strings(&["//a:a", "//b:b"])),
            ("by os", |c| {
                if c.get(OS) == "linux" {
                    strings(&["//c:c", "//l:l"])
                } else {
                    strings(&["//c:c"])
                }
            }),
            ("only some", |c| {
                if *c == config("linux", "arm64") {
                    strings(&["//la:la"])
                } else {
                    vec![]
                }
            }),
            ("by platform", |c| {
                vec![
                    "//c:c".to_string(),
                    format!("//{}:{}", c.get(OS), c.get(CPU)),
                ]
            }),
        ]
    }

    /// Whether `a` and `b` hold the same labels, in any order
    fn same_set(a: &[String], b: &[String]) -> bool {
        a.len() == b.len() && a.iter().all(|s| b.contains(s))
    }

    /// What labels_value writes reads back as every configuration's
    /// labels.
    #[test]
    fn labels_value_round_trip() {
        let space = Space::new(&platforms(), "");
        for (name, labels) in labels_cases() {
            let written = labels_value(&space, labels)
                .map(|v| crate::render_indented(&v, "    "))
                .unwrap_or_default();
            let read = read_labels(&target(&written), "deps", &space)
                .unwrap_or_else(|err| panic!("{name}: reading back {written}: {err}"));
            for config in &space.configurations {
                assert!(
                    same_set(&read.labels(config), &labels(config)),
                    "{name}, {config}: read back {:?}",
                    read.labels(config)
                );
            }
        }
    }

    /// What set_labels writes reads back as every configuration's labels,
    /// over a space with a Go build tag too.
    #[test]
    fn set_labels_round_trip() {
        let integration = OnOff {
            name: "go_tag:integration".into(),
            constraint: "prelude//go/tags/constraints:integration".into(),
            token: "integration".into(),
        };
        let space = Space::new(&platforms(), "").with_dimensions(&[integration]);
        let mut cases = labels_cases();
        cases.push(("by tag and os", |c| {
            if c.get(OS) == "linux" && c.get("go_tag:integration") == SET {
                strings(&["//it:it"])
            } else {
                vec![]
            }
        }));
        for (name, labels) in cases {
            let mut f = file(r#"["//old:old"]"#);
            set_labels(&mut f.targets[0], "deps", &space, labels);
            let written = parse("rules.star", f.write()).unwrap();
            let read = read_labels(&written.targets[0], "deps", &space)
                .unwrap_or_else(|err| panic!("{name}: reading back: {err}"));
            for config in &space.configurations {
                assert!(
                    same_set(&read.labels(config), &labels(config)),
                    "{name}, {config}: read back {:?}",
                    read.labels(config)
                );
            }
        }
    }

    /// set_labels writes the plain part into the auto-managed section,
    /// keeping the preserved one.
    #[test]
    fn set_labels_keeps_markers() {
        let space = Space::new(&platforms(), "");
        let mut f = file(
            "[\n        # turnkey:auto-start\n        \"//old:old\",\n        # turnkey:auto-end\n        # turnkey:preserve-start\n        \"//p:p\",\n        # turnkey:preserve-end\n    ]",
        );
        set_labels(&mut f.targets[0], "deps", &space, labels_cases()[2].1);
        let got = f.write();
        for want in [
            "turnkey:auto-start",
            "\"//c:c\"",
            "turnkey:preserve-start",
            "\"//p:p\"",
            "select(",
        ] {
            assert!(
                got.contains(want),
                "written rules.star lacks {want}:\n{got}"
            );
        }
        assert!(!got.contains("//old:old"), "still has //old:old:\n{got}");
    }

    /// A label list given as an expression, or as a select() whose keys
    /// the space doesn't know, isn't read: writing back would replace it.
    #[test]
    fn read_labels_unreadable() {
        let space = Space::new(&platforms(), "");
        for (deps, want) in [
            ("", true),
            (r#"["//a:a"]"#, true),
            (
                "[\n        # turnkey:auto-start\n        \"//a:a\",\n        # turnkey:auto-end\n    ]",
                true,
            ),
            ("_DEPS", false),
            (r#"_DEPS + ["//a:a"]"#, false),
            (r#"["//a:a"] if X else []"#, false),
            (r#""//a:a""#, false),
            (
                r#"["//a:a"] + select({"config//os:linux": ["//l:l"], "config//os:macos": []})"#,
                true,
            ),
            (
                r#"select({"config//os:linux": ["//l:l"], "DEFAULT": []})"#,
                true,
            ),
            (r#"select({"//my:setting": ["//l:l"]})"#, false),
            (r#"select({"config//os:linux": _LINUX})"#, false),
            (r#"select({"config//os:linux": True})"#, false),
            (r#"["//a:a"] + select({"config//os:linux": _LINUX})"#, false),
        ] {
            let got = read_labels(&target(deps), "deps", &space);
            assert_eq!(got.is_ok(), want, "deps = {deps}: {:?}", got.err());
        }
    }

    /// read takes any value that isn't a select() as is, and a select()
    /// alone of other values than lists; it refuses unknown keys, and a
    /// plain part followed by anything but a list.
    #[test]
    fn read_unreadable() {
        let space = Space::new(&platforms(), "");
        for (value, want) in [
            ("_DEPS", true),
            ("True", true),
            (
                r#"select({"config//os:linux": False, "DEFAULT": True})"#,
                true,
            ),
            (r#"select({"config//os:linux": _LINUX})"#, true),
            (r#"select({"//my:setting": True})"#, false),
            (r#"["//a:a"] + select({"config//os:linux": _LINUX})"#, false),
            (
                r#"["//a:a"] + select({"config//os:linux": "//l:l"})"#,
                false,
            ),
        ] {
            let got = read(&target(value), "deps", &space);
            assert_eq!(got.is_ok(), want, "{value}: {:?}", got.err());
        }
    }

    fn list(labels: &[&str]) -> Option<Value> {
        Some(Value::StringList(strings(labels)))
    }

    /// The policy for each configuration: the branch that applies is the
    /// most specific matching key, else DEFAULT; the plain part comes first
    /// and each label is kept once; with no branch applying, the plain part
    /// alone.
    #[test]
    fn read_policy() {
        let space = Space::new(&platforms(), "");
        let (linux_x86, linux_arm, macos_arm) = (
            config("linux", "x86_64"),
            config("linux", "arm64"),
            config("macos", "arm64"),
        );
        for (value, config, want) in [
            // absent
            ("", &linux_x86, None),
            // not a select()
            ("True", &linux_x86, Some(Value::Bool(true))),
            (
                r#"["//b:b", "//b:b"]"#,
                &linux_x86,
                list(&["//b:b", "//b:b"]),
            ),
            // DEFAULT, and the most specific key
            (
                r#"select({"config//os:linux": ["//l:l"], "DEFAULT": ["//d:d"]})"#,
                &macos_arm,
                list(&["//d:d"]),
            ),
            (
                r#"select({"config//os:linux": ["//l:l"], "DEFAULT": ["//d:d"]})"#,
                &linux_arm,
                list(&["//l:l"]),
            ),
            (
                r#"select({"config//os:linux": ["//l:l"], "toolchains//conditions:linux-arm64": ["//la:la"]})"#,
                &linux_arm,
                list(&["//la:la"]),
            ),
            // the plain part first, each label once
            (
                r#"["//c:c", "//l:l"] + select({"config//os:linux": ["//l:l", "//x:x"]})"#,
                &linux_x86,
                list(&["//c:c", "//l:l", "//x:x"]),
            ),
            (
                r#"select({"config//os:linux": ["//l:l", "//l:l"]})"#,
                &linux_x86,
                list(&["//l:l"]),
            ),
            // no branch applies: the plain part alone
            (
                r#"["//c:c"] + select({"config//os:linux": ["//l:l"]})"#,
                &macos_arm,
                list(&["//c:c"]),
            ),
            (
                r#"select({"config//os:linux": ["//l:l"]})"#,
                &macos_arm,
                None,
            ),
            (r#"select({"config//os:linux": False})"#, &macos_arm, None),
            // a select() alone of other values
            (
                r#"select({"config//os:linux": False, "DEFAULT": True})"#,
                &linux_x86,
                Some(Value::Bool(false)),
            ),
        ] {
            let read =
                read(&target(value), "deps", &space).unwrap_or_else(|err| panic!("{value}: {err}"));
            assert_eq!(read.value(config), want, "{value} in {config}");
        }
    }
}
