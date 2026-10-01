//! A conditional attribute: a target attribute whose value depends on the
//! build configuration, written as a plain value or as
//! `[<labels>] + select({<key>: <value>, ...})` over a [`Space`]
//!
//! It is the one place that decides how a value per configuration is
//! written, so the cell generators and rules sync agree on it. Reading one
//! back from a parsed `rules.star` is rules-star's `conditional` module.

use crate::{Space, Split};
use deps_gen_kit::starlark::{SelectBranch, SelectValue, Value};

/// The value of a label-list attribute that has `labels(config)` in each
/// configuration of `space`: the labels every configuration has, as a
/// plain list, followed by a `select()` of the others when they differ,
/// keyed as [`Space::split`] keys them. `None` when no configuration has a
/// label.
pub fn labels_value(
    space: &Space,
    labels: impl FnMut(&crate::Configuration) -> Vec<String>,
) -> Option<Value> {
    let split = space.split(labels);
    if !split.is_conditional() {
        if split.common.is_empty() {
            return None;
        }
        return Some(Value::StringList(split.common));
    }
    let branches = branches(&split);
    let common =
        (!split.common.is_empty()).then(|| Box::new(Value::StringList(split.common.clone())));
    Some(Value::Select(SelectValue { common, branches }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CPU, Configuration, OS, Platform};
    use deps_gen_kit::starlark::render_indented;

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
                if c.to_string() == "cpu=arm64,os=linux" {
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

    /// A label-list value's labels in a configuration, as rules sync reads
    /// them back: the plain part followed by the branch that applies, each
    /// label once
    fn read(value: Option<&Value>, space: &Space, config: &Configuration) -> Vec<String> {
        match value {
            None => vec![],
            Some(Value::StringList(labels)) => labels.clone(),
            Some(Value::Select(sel)) => {
                let keys: Vec<&str> = sel.branches.iter().map(|b| b.key.as_str()).collect();
                let matcher = space.matcher(&keys).expect("keys the space writes");
                let mut labels = match sel.common.as_deref() {
                    Some(Value::StringList(common)) => common.clone(),
                    None => vec![],
                    Some(other) => panic!("the plain part is {other:?}"),
                };
                if let Some(i) = matcher.branch(config) {
                    match &sel.branches[i].value {
                        Value::StringList(extra) => labels.extend(extra.iter().cloned()),
                        other => panic!("a branch is {other:?}"),
                    }
                }
                crate::dedupe(labels)
            }
            Some(other) => panic!("not a label list: {other:?}"),
        }
    }

    /// What labels_value writes reads back as every configuration's labels
    /// (the round trip of Go's conditional TestLabelsValueRoundTrip,
    /// read back from the value rather than reparsed).
    #[test]
    fn labels_value_round_trip() {
        let space = Space::new(&platforms(), "");
        for (name, labels) in labels_cases() {
            let value = labels_value(&space, labels);
            for config in &space.configurations {
                let mut got = read(value.as_ref(), &space, config);
                let mut want = labels(config);
                got.sort();
                want.sort();
                assert_eq!(got, want, "{name}, {config}");
            }
        }
    }

    /// No configuration having a label is no value, for cell generators to
    /// leave the attribute out.
    #[test]
    fn labels_value_of_nothing_is_none() {
        assert_eq!(
            labels_value(&Space::new(&platforms(), ""), |_| vec![]),
            None
        );
    }

    #[test]
    fn labels_value_written() {
        let space = Space::new(&platforms(), "");
        let value = labels_value(&space, |c| {
            if c.get(OS) == "linux" {
                strings(&["//c:c", "//l:l"])
            } else {
                strings(&["//c:c"])
            }
        });
        assert_eq!(
            render_indented(&value.unwrap(), "    "),
            r#"["//c:c"] + select({
        "config//os:linux": ["//l:l"],
        "config//os:macos": [],
    })"#
        );

        // Without common labels, a select() alone
        let value = labels_value(&space, |c| {
            if c.get(CPU) == "arm64" {
                strings(&["//a:a"])
            } else {
                vec![]
            }
        });
        assert_eq!(
            render_indented(&value.unwrap(), "    "),
            r#"select({
        "config//cpu:arm64": ["//a:a"],
        "config//cpu:x86_64": [],
    })"#
        );
    }
}
